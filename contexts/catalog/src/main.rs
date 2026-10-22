use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use gavel_catalog::{
    adapters::{
        inbound::{
            graphql::{CatalogGraphqlApi, schema},
            grpc::{CatalogGrpcApi, CatalogGrpcService},
            workload_identity::WorkloadIdentityInterceptor,
        },
        outbound::{
            authz::AuthzGrpcAdapter,
            postgres::PostgresCatalogStore,
            postgres_reader::PostgresPublicReader,
            s3::{S3Config, S3ObjectStorage},
        },
    },
    application::{
        ApplicationError, CatalogReader, CleanupWorker, Clock, CompleteLotImageUpload,
        CompleteUploadInput, GrantReconciliationWorker, IdGenerator, ListLot, ListLotInput,
        LotMutationResult, LotPage, LotQuery, PublicLot, RequestLotImageUpload, RequestUploadInput,
        UpdateLot, UpdateLotInput, UploadCompletedResult, UploadIntentResult, WithdrawLot,
        WithdrawLotInput,
    },
    bootstrap::{BackgroundWorker, RuntimeConfig, RuntimeState, serve, unix_millis},
    domain::{ImageId, LotId},
    telemetry::{CatalogTelemetry, Operation, Outcome, record, set_backlogs},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct SystemClock;
impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        unix_millis()
    }
}

struct UuidIds;
impl IdGenerator for UuidIds {
    fn image_id(&self) -> Result<ImageId, gavel_catalog::application::PortError> {
        ImageId::new(Uuid::now_v7().to_string())
            .map_err(|_| gavel_catalog::application::PortError::Internal)
    }
    fn object_key(
        &self,
        image_id: &ImageId,
    ) -> Result<String, gavel_catalog::application::PortError> {
        Ok(format!("catalog/{}", image_id.as_str()))
    }
    fn event_id(&self) -> Result<String, gavel_catalog::application::PortError> {
        Ok(Uuid::now_v7().to_string())
    }
}

struct CatalogApplication {
    store: Arc<PostgresCatalogStore>,
    reader: Arc<PostgresPublicReader>,
    authz: Arc<AuthzGrpcAdapter>,
    storage: Arc<S3ObjectStorage>,
    clock: Arc<SystemClock>,
    ids: Arc<UuidIds>,
    telemetry: Arc<CatalogTelemetry>,
    fingerprint_key: Vec<u8>,
    dependency_timeout_millis: u64,
    upload_ttl_millis: u64,
}

impl CatalogApplication {
    async fn observe<T>(
        operation: Operation,
        started: Instant,
        result: Result<T, ApplicationError>,
    ) -> Result<T, ApplicationError> {
        let outcome = result
            .as_ref()
            .map_or_else(Outcome::from_error, |_| Outcome::Success);
        record(
            operation,
            outcome,
            started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
        );
        result
    }
}

#[async_trait]
impl CatalogGraphqlApi for CatalogApplication {
    async fn lot(&self, lot_id: &str) -> Result<Option<PublicLot>, ApplicationError> {
        let started = Instant::now();
        let result = async {
            let id =
                LotId::new(lot_id.to_owned()).map_err(|_| ApplicationError::InvalidArgument {
                    reason: "invalid_lot_id",
                    safe_message: "lot id is invalid".to_owned(),
                })?;
            self.reader.lot(&id).await
        }
        .await;
        Self::observe(Operation::LotQuery, started, result).await
    }

    async fn lots(&self, first: usize, after: Option<String>) -> Result<LotPage, ApplicationError> {
        let started = Instant::now();
        let result = self.reader.lots(LotQuery { first, after }).await;
        Self::observe(Operation::LotsQuery, started, result).await
    }

    async fn request_upload(
        &self,
        input: RequestUploadInput,
    ) -> Result<UploadIntentResult, ApplicationError> {
        RequestLotImageUpload::new(
            self.store.as_ref(),
            self.storage.as_ref(),
            self.clock.as_ref(),
            self.ids.as_ref(),
            self.telemetry.as_ref(),
            self.upload_ttl_millis,
            self.dependency_timeout_millis,
        )
        .execute(input)
        .await
    }

    async fn complete_upload(
        &self,
        input: CompleteUploadInput,
    ) -> Result<UploadCompletedResult, ApplicationError> {
        CompleteLotImageUpload::new(
            self.store.as_ref(),
            self.storage.as_ref(),
            self.telemetry.as_ref(),
            self.dependency_timeout_millis,
        )
        .execute(input)
        .await
    }

    async fn list_lot(&self, input: ListLotInput) -> Result<PublicLot, ApplicationError> {
        let started = Instant::now();
        let result = async {
            let lot_id = input.lot_id.clone();
            ListLot::new(
                self.store.as_ref(),
                &self.fingerprint_key,
                self.authz.as_ref(),
                self.store.as_ref(),
                self.ids.as_ref(),
                self.dependency_timeout_millis,
            )
            .execute(input)
            .await?;
            self.lot(&lot_id).await?.ok_or(ApplicationError::Internal)
        }
        .await;
        Self::observe(Operation::ListLot, started, result).await
    }

    async fn update_lot(&self, input: UpdateLotInput) -> Result<PublicLot, ApplicationError> {
        let started = Instant::now();
        let result = async {
            let lot_id = input.lot_id.clone();
            UpdateLot::new(
                self.store.as_ref(),
                &self.fingerprint_key,
                self.authz.as_ref(),
                self.store.as_ref(),
                self.ids.as_ref(),
                self.dependency_timeout_millis,
            )
            .execute(input)
            .await?;
            self.lot(&lot_id).await?.ok_or(ApplicationError::Internal)
        }
        .await;
        Self::observe(Operation::UpdateLot, started, result).await
    }
}

#[async_trait]
impl CatalogGrpcApi for CatalogApplication {
    async fn withdraw(
        &self,
        input: WithdrawLotInput,
    ) -> Result<LotMutationResult, ApplicationError> {
        let started = Instant::now();
        let result = WithdrawLot::new(
            self.store.as_ref(),
            &self.fingerprint_key,
            self.authz.as_ref(),
            self.store.as_ref(),
            self.ids.as_ref(),
            self.dependency_timeout_millis,
        )
        .execute(input)
        .await;
        Self::observe(Operation::WithdrawLot, started, result).await
    }
}

struct CleanupRuntime {
    store: Arc<PostgresCatalogStore>,
    storage: Arc<S3ObjectStorage>,
    telemetry: Arc<CatalogTelemetry>,
    interval: Duration,
    lease_millis: u64,
    ready_ttl_millis: u64,
    dependency_timeout_millis: u64,
}

#[async_trait]
impl BackgroundWorker for CleanupRuntime {
    fn name(&self) -> &'static str {
        "media-cleanup"
    }
    async fn run(&self, shutdown: CancellationToken) -> Result<(), String> {
        let worker = CleanupWorker::new(
            self.store.as_ref(),
            self.storage.as_ref(),
            self.telemetry.as_ref(),
            self.dependency_timeout_millis,
        );
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = tokio::time::sleep(self.interval) => {
                    worker.run_batch("catalog-runtime", unix_millis(), self.ready_ttl_millis, self.lease_millis, 100)
                        .await.map_err(|error| error.to_string())?;
                }
            }
        }
    }
}

struct GrantRuntime {
    store: Arc<PostgresCatalogStore>,
    authz: Arc<AuthzGrpcAdapter>,
    interval: Duration,
    lease_millis: u64,
    dependency_timeout_millis: u64,
}

struct MetricsRuntime {
    pool: PgPool,
    interval: Duration,
}

#[async_trait]
impl BackgroundWorker for MetricsRuntime {
    fn name(&self) -> &'static str {
        "operational-metrics"
    }
    async fn run(&self, shutdown: CancellationToken) -> Result<(), String> {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = tokio::time::sleep(self.interval) => {
                    let pending_age: i64 = sqlx::query_scalar(
                        "SELECT COALESCE(EXTRACT(EPOCH FROM (clock_timestamp() - MIN(created_at)))::bigint, 0) \
                         FROM catalog_lot_images WHERE state <> 'ATTACHED'"
                    ).fetch_one(&self.pool).await.map_err(|_| "catalog database metrics unavailable".to_owned())?;
                    let outbox: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog_outbox WHERE published_at IS NULL")
                        .fetch_one(&self.pool).await.map_err(|_| "catalog database metrics unavailable".to_owned())?;
                    let reconciliation: i64 = sqlx::query_scalar(
                        "SELECT count(*) FROM catalog_reconciliation_queue WHERE completed_at IS NULL"
                    ).fetch_one(&self.pool).await.map_err(|_| "catalog database metrics unavailable".to_owned())?;
                    set_backlogs(
                        u64::try_from(pending_age.max(0)).unwrap_or(0),
                        u64::try_from(outbox.max(0)).unwrap_or(0),
                        u64::try_from(reconciliation.max(0)).unwrap_or(0),
                    );
                }
            }
        }
    }
}

#[async_trait]
impl BackgroundWorker for GrantRuntime {
    fn name(&self) -> &'static str {
        "authz-grant-reconciliation"
    }
    async fn run(&self, shutdown: CancellationToken) -> Result<(), String> {
        let worker = GrantReconciliationWorker::new(
            self.store.as_ref(),
            self.authz.as_ref(),
            self.dependency_timeout_millis,
        );
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = tokio::time::sleep(self.interval) => {
                    let started = Instant::now();
                    let result = worker.run_batch("catalog-runtime", unix_millis(), self.lease_millis, 100).await;
                    let outcome = result.as_ref().map_or_else(Outcome::from_error, |_| Outcome::Success);
                    record(Operation::GrantReconciliation, outcome, started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64);
                    result.map_err(|error| error.to_string())?;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gavel_catalog=info".into()),
        )
        .init();
    if std::env::args().nth(1).as_deref() == Some("migrate")
        || std::env::args().nth(1).as_deref() == Some("--migrate")
    {
        let database_url = std::env::var("CATALOG_DATABASE_URL")?;
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        return Ok(());
    }
    let config = RuntimeConfig::from_env()?;
    let pool: PgPool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(config.dependency_timeout)
        .connect(&config.database_url)
        .await?;
    sqlx::query("SELECT 1").execute(&pool).await?;
    let store = Arc::new(PostgresCatalogStore::new(pool.clone()));
    let reader = Arc::new(PostgresPublicReader::new(
        pool.clone(),
        config.cursor_key.clone(),
        config.image_delivery_base_url.clone(),
    ));
    let authz = Arc::new(
        AuthzGrpcAdapter::connect(&config.authz_url, config.dependency_timeout)
            .await
            .map_err(|_| std::io::Error::other("Authz connection failed"))?,
    );
    authz
        .ready()
        .await
        .map_err(|_| std::io::Error::other("Authz is not ready"))?;
    let storage = Arc::new(
        S3ObjectStorage::new(S3Config {
            endpoint: config.s3_endpoint.clone(),
            region: config.s3_region.clone(),
            bucket: config.s3_bucket.clone(),
            access_key: config.s3_access_key.clone(),
            secret_key: config.s3_secret_key.clone(),
            session_token: None,
            path_style: config.s3_path_style,
        })
        .map_err(std::io::Error::other)?,
    );
    storage
        .ready(config.dependency_timeout)
        .await
        .map_err(|_| std::io::Error::other("object storage is not ready"))?;
    let clock = Arc::new(SystemClock);
    let telemetry = Arc::new(CatalogTelemetry);
    let dependency_timeout_millis = u64::try_from(config.dependency_timeout.as_millis())?;
    let app = Arc::new(CatalogApplication {
        store: Arc::clone(&store),
        reader,
        authz: Arc::clone(&authz),
        storage: Arc::clone(&storage),
        clock: Arc::clone(&clock),
        ids: Arc::new(UuidIds),
        telemetry: Arc::clone(&telemetry),
        fingerprint_key: config.fingerprint_key.clone(),
        dependency_timeout_millis,
        upload_ttl_millis: u64::try_from(config.upload_ttl.as_millis())?,
    });
    let interceptor = if config.enable_test_processes_identity {
        WorkloadIdentityInterceptor::test_processes(true).map_err(std::io::Error::other)?
    } else {
        WorkloadIdentityInterceptor::production(
            config.spiffe_prefix.clone(),
            config.trusted_proxies.clone(),
        )
        .map_err(std::io::Error::other)?
    };
    let http = TcpListener::bind(config.http_addr).await?;
    let grpc = TcpListener::bind(config.grpc_addr).await?;
    let shutdown = CancellationToken::new();
    install_shutdown(shutdown.clone());
    serve(
        http,
        grpc,
        schema(Arc::clone(&app) as Arc<dyn CatalogGraphqlApi>),
        CatalogGrpcService::new(
            app as Arc<dyn CatalogGrpcApi>,
            clock,
            config.dependency_timeout,
        )
        .map_err(std::io::Error::other)?,
        interceptor,
        vec![
            Arc::new(CleanupRuntime {
                store: Arc::clone(&store),
                storage,
                telemetry,
                interval: config.worker_interval,
                lease_millis: u64::try_from(config.worker_lease.as_millis())?,
                ready_ttl_millis: u64::try_from(config.ready_image_ttl.as_millis())?,
                dependency_timeout_millis,
            }),
            Arc::new(GrantRuntime {
                store,
                authz,
                interval: config.worker_interval,
                lease_millis: u64::try_from(config.worker_lease.as_millis())?,
                dependency_timeout_millis,
            }),
            Arc::new(MetricsRuntime {
                pool,
                interval: config.worker_interval,
            }),
        ],
        Arc::new(RuntimeState::default()),
        config.trusted_proxies,
        shutdown,
    )
    .await?;
    Ok(())
}

fn install_shutdown(token: CancellationToken) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler must install");
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
        token.cancel();
    });
}
