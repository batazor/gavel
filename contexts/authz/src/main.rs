use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use gavel_authz::{
    adapters::{
        inbound::{AuthzGrpcService, WorkloadIdentityInterceptor},
        outbound::{PostgresReceiptStore, SpiceDbAdapter},
    },
    bootstrap::{
        Config, HealthState, ProbeError, ReceiptWorkerProbe, run_receipt_worker, serve_http,
    },
    rpc::authz_service_server::AuthzServiceServer,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::{net::TcpListener, task::JoinSet};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;

#[derive(Clone)]
struct RuntimeProbe {
    pool: PgPool,
    spicedb: SpiceDbAdapter,
    timeout: Duration,
}

#[async_trait]
impl ReceiptWorkerProbe for RuntimeProbe {
    async fn probe(&self) -> Result<(), ProbeError> {
        tokio::time::timeout(
            self.timeout,
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM authz_command_receipts \
                 WHERE status = 'PENDING' AND lease_expires_at <= clock_timestamp()",
            )
            .fetch_one(&self.pool),
        )
        .await
        .map_err(|_| ProbeError::Unavailable)?
        .map_err(|_| ProbeError::Unavailable)?;
        self.spicedb
            .ready()
            .await
            .map_err(|_| ProbeError::Unavailable)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gavel_authz=info".into()),
        )
        .init();
    if std::env::args().nth(1).as_deref() == Some("bootstrap") {
        let report = gavel_authz::local_bootstrap::run_from_env().await?;
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--migrate") {
        let database_url = std::env::var("AUTHZ_DATABASE_URL")?;
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        return Ok(());
    }
    let config = Config::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(config.dependency_timeout)
        .connect(&config.database_url)
        .await?;
    let spicedb = SpiceDbAdapter::connect(
        &config.spicedb_endpoint,
        &config.spicedb_credential,
        config.dependency_timeout,
    )
    .await?;
    let receipt_store = Arc::new(PostgresReceiptStore::new(
        pool.clone(),
        config.receipt_lease,
    )?);
    let service = AuthzGrpcService::new(
        Arc::new(spicedb.clone()),
        receipt_store,
        config.principal_key,
        config.fingerprint_key,
        config.dependency_timeout,
    )?;
    let interceptor = match config.test_identity {
        Some(identity) => WorkloadIdentityInterceptor::test(identity, true)?,
        None => WorkloadIdentityInterceptor::production(
            config.spiffe_prefix.clone(),
            config.trusted_proxies.clone(),
        )?,
    };

    let http_listener = TcpListener::bind(config.http_addr).await?;
    let grpc_listener = TcpListener::bind(config.grpc_addr).await?;
    let state = HealthState::start();
    let shutdown = CancellationToken::new();
    let mut tasks = JoinSet::new();
    tasks.spawn({
        let state = state.clone();
        let shutdown = shutdown.clone();
        async move {
            serve_http(http_listener, state, shutdown)
                .await
                .map_err(|error| error.to_string())
        }
    });
    tasks.spawn({
        let shutdown = shutdown.clone();
        async move {
            Server::builder()
                .add_service(AuthzServiceServer::with_interceptor(service, interceptor))
                .serve_with_incoming_shutdown(
                    TcpListenerStream::new(grpc_listener),
                    shutdown.cancelled_owned(),
                )
                .await
                .map_err(|error| error.to_string())
        }
    });
    tasks.spawn({
        let state = state.clone();
        let shutdown = shutdown.clone();
        let probe = RuntimeProbe {
            pool,
            spicedb,
            timeout: config.dependency_timeout,
        };
        async move {
            run_receipt_worker(&probe, state, config.worker_interval, shutdown)
                .await
                .map_err(|error| error.to_string())
        }
    });

    let failure = tokio::select! {
        signal = shutdown_signal() => {
            signal?;
            None
        }
        result = tasks.join_next() => {
            Some(match result {
                Some(Ok(Ok(()))) => "runtime component stopped unexpectedly".to_owned(),
                Some(Ok(Err(error))) => error,
                Some(Err(error)) => error.to_string(),
                None => "runtime has no components".to_owned(),
            })
        }
    };
    state.stop();
    shutdown.cancel();
    while let Some(result) = tasks.join_next().await {
        if let Err(error) = result {
            tracing::error!(error = %error, "runtime component join failed");
        }
    }
    if let Some(error) = failure {
        return Err(error.into());
    }
    Ok(())
}

async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}
