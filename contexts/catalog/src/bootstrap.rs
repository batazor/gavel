//! Tokio runtime composition and process lifecycle.

use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use async_trait::async_trait;
use axum::{
    Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Serialize;
use tokio::{net::TcpListener, task::JoinSet};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;

use crate::{
    adapters::inbound::{
        graphql::{CatalogSchema, VerifiedRequestContext},
        grpc::CatalogGrpcService,
        workload_identity::{TrustedProxyCidr, WorkloadIdentityInterceptor},
    },
    adapters::outbound::local_storage::{LocalObjectStorage, LocalUploadError},
    application::Principal,
    rpc::catalog_service_server::CatalogServiceServer,
};

#[derive(Clone)]
pub struct RuntimeConfig {
    pub http_addr: SocketAddr,
    pub grpc_addr: SocketAddr,
    pub database_url: String,
    pub authz_url: String,
    pub s3_endpoint: url::Url,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_path_style: bool,
    pub fingerprint_key: Vec<u8>,
    pub cursor_key: Vec<u8>,
    pub image_delivery_base_url: String,
    pub dependency_timeout: Duration,
    pub worker_interval: Duration,
    pub worker_lease: Duration,
    pub ready_image_ttl: Duration,
    pub upload_ttl: Duration,
    pub spiffe_prefix: String,
    pub trusted_proxies: Vec<TrustedProxyCidr>,
    pub enable_test_processes_identity: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("required environment variable {0} is missing")]
    Missing(&'static str),
    #[error("environment variable {0} is invalid")]
    Invalid(&'static str),
}

impl RuntimeConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            http_addr: socket_addr("CATALOG_HTTP_ADDR", 8080)?,
            grpc_addr: socket_addr("CATALOG_GRPC_ADDR", 9090)?,
            database_url: required("CATALOG_DATABASE_URL")?,
            authz_url: required("CATALOG_AUTHZ_URL")?,
            s3_endpoint: required("CATALOG_S3_ENDPOINT")?
                .parse()
                .map_err(|_| ConfigError::Invalid("CATALOG_S3_ENDPOINT"))?,
            s3_region: required("CATALOG_S3_REGION")?,
            s3_bucket: required("CATALOG_S3_BUCKET")?,
            s3_access_key: required("CATALOG_S3_ACCESS_KEY")?,
            s3_secret_key: required("CATALOG_S3_SECRET_KEY")?,
            s3_path_style: boolean("CATALOG_S3_PATH_STYLE", true)?,
            fingerprint_key: secret("CATALOG_FINGERPRINT_HMAC_KEY")?,
            cursor_key: secret("CATALOG_CURSOR_HMAC_KEY")?,
            image_delivery_base_url: required("CATALOG_IMAGE_DELIVERY_BASE_URL")?,
            dependency_timeout: millis("CATALOG_DEPENDENCY_TIMEOUT_MS", 500)?,
            worker_interval: millis("CATALOG_WORKER_INTERVAL_MS", 1_000)?,
            worker_lease: millis("CATALOG_WORKER_LEASE_MS", 30_000)?,
            ready_image_ttl: millis("CATALOG_READY_IMAGE_TTL_MS", 86_400_000)?,
            upload_ttl: millis("CATALOG_UPLOAD_TTL_MS", 900_000)?,
            spiffe_prefix: required("CATALOG_SPIFFE_PREFIX")?,
            trusted_proxies: env::var("CATALOG_TRUSTED_PROXY_CIDRS")
                .unwrap_or_default()
                .split(',')
                .filter(|value| !value.trim().is_empty())
                .map(|value| {
                    value
                        .trim()
                        .parse()
                        .map_err(|_| ConfigError::Invalid("CATALOG_TRUSTED_PROXY_CIDRS"))
                })
                .collect::<Result<_, _>>()?,
            enable_test_processes_identity: boolean(
                "CATALOG_ENABLE_TEST_PROCESSES_IDENTITY",
                false,
            )?,
        })
    }
}

#[derive(Default)]
pub struct RuntimeState {
    ready: AtomicBool,
}

impl RuntimeState {
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
    pub fn set_ready(&self, value: bool) {
        self.ready.store(value, Ordering::Release);
    }
}

#[async_trait]
pub trait BackgroundWorker: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(&self, shutdown: CancellationToken) -> Result<(), String>;
}

#[derive(Clone)]
struct HttpState {
    schema: CatalogSchema,
    runtime: Arc<RuntimeState>,
    trusted_proxies: Arc<Vec<TrustedProxyCidr>>,
    local_storage: Option<Arc<LocalObjectStorage>>,
}

pub fn http_router(
    schema: CatalogSchema,
    runtime: Arc<RuntimeState>,
    trusted_proxies: Vec<TrustedProxyCidr>,
) -> Router {
    http_router_with_local_storage(schema, runtime, trusted_proxies, None)
}

pub fn http_router_with_local_storage(
    schema: CatalogSchema,
    runtime: Arc<RuntimeState>,
    trusted_proxies: Vec<TrustedProxyCidr>,
    local_storage: Option<Arc<LocalObjectStorage>>,
) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .route("/graphql", post(graphql))
        .route("/images/{image_id}", get(local_image))
        .route(
            "/local/object-storage/{token}",
            put(local_upload).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .with_state(HttpState {
            schema,
            runtime,
            trusted_proxies: Arc::new(trusted_proxies),
            local_storage,
        })
}

#[allow(clippy::too_many_arguments)]
pub async fn serve(
    http_listener: TcpListener,
    grpc_listener: TcpListener,
    schema: CatalogSchema,
    grpc_service: CatalogGrpcService,
    interceptor: WorkloadIdentityInterceptor,
    workers: Vec<Arc<dyn BackgroundWorker>>,
    runtime: Arc<RuntimeState>,
    trusted_proxies: Vec<TrustedProxyCidr>,
    shutdown: CancellationToken,
) -> Result<(), RuntimeError> {
    serve_with_local_storage(
        http_listener,
        grpc_listener,
        schema,
        grpc_service,
        interceptor,
        workers,
        runtime,
        trusted_proxies,
        None,
        shutdown,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn serve_with_local_storage(
    http_listener: TcpListener,
    grpc_listener: TcpListener,
    schema: CatalogSchema,
    grpc_service: CatalogGrpcService,
    interceptor: WorkloadIdentityInterceptor,
    workers: Vec<Arc<dyn BackgroundWorker>>,
    runtime: Arc<RuntimeState>,
    trusted_proxies: Vec<TrustedProxyCidr>,
    local_storage: Option<Arc<LocalObjectStorage>>,
    shutdown: CancellationToken,
) -> Result<(), RuntimeError> {
    let app = http_router_with_local_storage(
        schema,
        Arc::clone(&runtime),
        trusted_proxies,
        local_storage,
    );
    let server_shutdown = shutdown.child_token();
    let mut tasks = JoinSet::new();
    let http_shutdown = server_shutdown.clone();
    tasks.spawn(async move {
        axum::serve(
            http_listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(http_shutdown.cancelled_owned())
        .await
        .map_err(|error| error.to_string())
    });
    let grpc_shutdown = server_shutdown.clone();
    tasks.spawn(async move {
        Server::builder()
            .add_service(CatalogServiceServer::with_interceptor(
                grpc_service,
                interceptor,
            ))
            .serve_with_incoming_shutdown(
                TcpListenerStream::new(grpc_listener),
                grpc_shutdown.cancelled_owned(),
            )
            .await
            .map_err(|error| error.to_string())
    });
    for worker in workers {
        let token = server_shutdown.clone();
        tasks.spawn(async move {
            worker
                .run(token)
                .await
                .map_err(|error| format!("{}: {error}", worker.name()))
        });
    }
    runtime.set_ready(true);
    let result = tokio::select! {
        () = shutdown.cancelled() => Ok(()),
        task = tasks.join_next() => match task {
            Some(Ok(Ok(()))) if shutdown.is_cancelled() => Ok(()),
            Some(Ok(Ok(()))) => Err(RuntimeError::ComponentStopped),
            Some(Ok(Err(error))) => Err(RuntimeError::ComponentFailed(error)),
            Some(Err(error)) => Err(RuntimeError::ComponentFailed(error.to_string())),
            None => Err(RuntimeError::ComponentStopped),
        }
    };
    runtime.set_ready(false);
    server_shutdown.cancel();
    while tasks.join_next().await.is_some() {}
    result
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("runtime component stopped unexpectedly")]
    ComponentStopped,
    #[error("runtime component failed: {0}")]
    ComponentFailed(String),
}

#[derive(Serialize)]
struct HealthBody<'a> {
    status: &'a str,
    service: &'a str,
}

async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        axum::Json(HealthBody {
            status: "ok",
            service: "catalog",
        }),
    )
}

async fn ready(State(state): State<HttpState>) -> Response {
    let (status, value) = if state.runtime.is_ready() {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not_ready")
    };
    (
        status,
        axum::Json(HealthBody {
            status: value,
            service: "catalog",
        }),
    )
        .into_response()
}

async fn metrics() -> impl IntoResponse {
    (StatusCode::OK, crate::telemetry::render_prometheus())
}

async fn local_upload(
    State(state): State<HttpState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(storage) = state.local_storage else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match storage.upload(&token, &headers, body).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(LocalUploadError::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Err(LocalUploadError::Expired) => StatusCode::GONE.into_response(),
        Err(LocalUploadError::MetadataMismatch) => StatusCode::UNPROCESSABLE_ENTITY.into_response(),
    }
}

async fn local_image(State(state): State<HttpState>, Path(image_id): Path<String>) -> Response {
    let Some(storage) = state.local_storage else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((content_type, body)) = storage.image(&image_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(content_type) = HeaderValue::from_str(&content_type) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=3600"),
            ),
        ],
        body,
    )
        .into_response()
}

async fn graphql(
    State(state): State<HttpState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> GraphQLResponse {
    let mut request = request.into_inner();
    if peer.ip().is_loopback()
        || state
            .trusted_proxies
            .iter()
            .any(|cidr| cidr.contains(peer.ip()))
    {
        if let Some(verified) = verified_http_context(&headers) {
            request = request.data(verified);
        }
    }
    state.schema.execute(request).await.into()
}

fn verified_http_context(headers: &HeaderMap) -> Option<VerifiedRequestContext> {
    let principal_id = headers.get("x-gavel-user-id")?.to_str().ok()?.trim();
    let correlation_id = headers.get("x-correlation-id")?.to_str().ok()?.trim();
    if principal_id.is_empty()
        || correlation_id.is_empty()
        || principal_id.len() > 128
        || correlation_id.len() > 256
    {
        return None;
    }
    Some(VerifiedRequestContext {
        principal: Principal {
            principal_id: principal_id.to_owned(),
        },
        correlation_id: correlation_id.to_owned(),
        now_millis: unix_millis(),
    })
}

pub fn unix_millis() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::Missing(name))
}

fn socket_addr(name: &'static str, port: u16) -> Result<SocketAddr, ConfigError> {
    env::var(name)
        .unwrap_or_else(|_| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port).to_string())
        .parse()
        .map_err(|_| ConfigError::Invalid(name))
}

fn millis(name: &'static str, default: u64) -> Result<Duration, ConfigError> {
    let value = env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse::<u64>()
        .map_err(|_| ConfigError::Invalid(name))?;
    if value == 0 {
        Err(ConfigError::Invalid(name))
    } else {
        Ok(Duration::from_millis(value))
    }
}

fn boolean(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    env::var(name).map_or(Ok(default), |value| {
        value.parse().map_err(|_| ConfigError::Invalid(name))
    })
}

fn secret(name: &'static str) -> Result<Vec<u8>, ConfigError> {
    let value = required(name)?.into_bytes();
    if value.len() < 32 {
        Err(ConfigError::Invalid(name))
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::inbound::{
            graphql::{CatalogGraphqlApi, schema},
            grpc::{CatalogGrpcApi, CatalogGrpcService},
            workload_identity::WorkloadIdentityInterceptor,
        },
        application::{
            ApplicationError, Clock, CompleteUploadInput, ListLotInput, LotMutationResult, LotPage,
            PublicLot, RequestUploadInput, UpdateLotInput, UploadCompletedResult,
            UploadIntentResult, WithdrawLotInput,
        },
    };

    struct EmptyApi;
    #[async_trait]
    impl CatalogGraphqlApi for EmptyApi {
        async fn lot(&self, _: &str) -> Result<Option<PublicLot>, ApplicationError> {
            Ok(None)
        }
        async fn lots(&self, _: usize, _: Option<String>) -> Result<LotPage, ApplicationError> {
            Ok(LotPage {
                lots: vec![],
                cursors: vec![],
                end_cursor: None,
                has_next_page: false,
            })
        }
        async fn request_upload(
            &self,
            _: RequestUploadInput,
        ) -> Result<UploadIntentResult, ApplicationError> {
            Err(ApplicationError::Internal)
        }
        async fn complete_upload(
            &self,
            _: CompleteUploadInput,
        ) -> Result<UploadCompletedResult, ApplicationError> {
            Err(ApplicationError::Internal)
        }
        async fn list_lot(&self, _: ListLotInput) -> Result<PublicLot, ApplicationError> {
            Err(ApplicationError::Internal)
        }
        async fn update_lot(&self, _: UpdateLotInput) -> Result<PublicLot, ApplicationError> {
            Err(ApplicationError::Internal)
        }
    }
    #[async_trait]
    impl CatalogGrpcApi for EmptyApi {
        async fn withdraw(
            &self,
            _: WithdrawLotInput,
        ) -> Result<LotMutationResult, ApplicationError> {
            Err(ApplicationError::Internal)
        }
    }
    struct FixedClock;
    impl Clock for FixedClock {
        fn now_millis(&self) -> u64 {
            1
        }
    }

    struct Standby;
    #[async_trait]
    impl BackgroundWorker for Standby {
        fn name(&self) -> &'static str {
            "standby"
        }
        async fn run(&self, shutdown: CancellationToken) -> Result<(), String> {
            shutdown.cancelled().await;
            Ok(())
        }
    }
    struct Failing;
    #[async_trait]
    impl BackgroundWorker for Failing {
        fn name(&self) -> &'static str {
            "failing-worker"
        }
        async fn run(&self, _: CancellationToken) -> Result<(), String> {
            Err("boom".to_owned())
        }
    }

    async fn runtime(
        workers: Vec<Arc<dyn BackgroundWorker>>,
        shutdown: CancellationToken,
        state: Arc<RuntimeState>,
    ) -> Result<(), RuntimeError> {
        let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let grpc = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = Arc::new(EmptyApi);
        serve(
            http,
            grpc,
            schema(Arc::clone(&api) as Arc<dyn CatalogGraphqlApi>),
            CatalogGrpcService::new(
                api as Arc<dyn CatalogGrpcApi>,
                Arc::new(FixedClock),
                Duration::from_secs(1),
            )
            .unwrap(),
            WorkloadIdentityInterceptor::test_processes(true).unwrap(),
            workers,
            state,
            vec![],
            shutdown,
        )
        .await
    }

    #[tokio::test]
    async fn readiness_tracks_startup_and_graceful_shutdown() {
        let shutdown = CancellationToken::new();
        let state = Arc::new(RuntimeState::default());
        let task = tokio::spawn(runtime(
            vec![Arc::new(Standby)],
            shutdown.clone(),
            Arc::clone(&state),
        ));
        for _ in 0..20 {
            if state.is_ready() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(state.is_ready());
        shutdown.cancel();
        task.await.unwrap().unwrap();
        assert!(!state.is_ready());
    }

    #[tokio::test]
    async fn fatal_worker_failure_stops_and_unreadies_runtime() {
        let state = Arc::new(RuntimeState::default());
        let result = runtime(
            vec![Arc::new(Failing)],
            CancellationToken::new(),
            Arc::clone(&state),
        )
        .await;
        assert!(
            matches!(result, Err(RuntimeError::ComponentFailed(message)) if message.contains("failing-worker"))
        );
        assert!(!state.is_ready());
    }

    #[test]
    fn config_primitives_fail_closed() {
        assert_eq!(secret_value("short"), Err(ConfigError::Invalid("TEST")));
        assert!(
            "10.0.0.0/8"
                .parse::<TrustedProxyCidr>()
                .unwrap()
                .contains("10.1.2.3".parse().unwrap())
        );
    }

    fn secret_value(value: &str) -> Result<Vec<u8>, ConfigError> {
        if value.len() < 32 {
            Err(ConfigError::Invalid("TEST"))
        } else {
            Ok(value.as_bytes().to_vec())
        }
    }
}
