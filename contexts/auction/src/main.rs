use std::{sync::Arc, time::Duration};

use gavel_auction::{
    adapters::{
        inbound::{
            graphql::{AuctionGraphqlApi, AuctionGraphqlApplication, schema},
            grpc::{AuctionGrpcApi, AuctionGrpcApplication, AuctionGrpcService, SystemClock},
        },
        outbound::{
            authz::AuthzGrpcAdapter, postgres::PostgresAuctionStore, redis::RedisPriceCache,
        },
    },
    bootstrap::{
        ReconciliationBackgroundWorker, RuntimeConfig, RuntimeState, StandbyWorker, serve,
    },
};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        let database_url = std::env::var("DATABASE_URL")?;
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        return Ok(());
    }
    let config = RuntimeConfig::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;
    let store = Arc::new(PostgresAuctionStore::new(pool));
    let authz =
        Arc::new(AuthzGrpcAdapter::connect(&config.authz_url, config.dependency_timeout).await?);
    let _redis = RedisPriceCache::connect(
        &config.redis_url,
        "gavel:auction:price:",
        config.dependency_timeout,
        3,
        Duration::from_secs(5),
        Duration::from_secs(300),
    )
    .await?;
    let graphql_api = Arc::new(AuctionGraphqlApplication::new(
        Arc::clone(&store),
        Arc::clone(&authz),
        Arc::clone(&store),
        3,
        config.fingerprint_key,
    ));
    let grpc_api = Arc::new(AuctionGrpcApplication::new(
        Arc::clone(&store),
        Arc::clone(&authz),
        3,
        config.fingerprint_key,
    ));
    let grpc_service =
        AuctionGrpcService::new(grpc_api as Arc<dyn AuctionGrpcApi>, Arc::new(SystemClock));
    let http_listener = TcpListener::bind(config.http_addr).await?;
    let grpc_listener = TcpListener::bind(config.grpc_addr).await?;
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler must install");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
        signal.cancel();
    });
    serve(
        http_listener,
        grpc_listener,
        schema(graphql_api as Arc<dyn AuctionGraphqlApi>),
        grpc_service,
        vec![
            Arc::new(StandbyWorker::new("catalog-consumer")),
            Arc::new(StandbyWorker::new("payments-consumer")),
            Arc::new(StandbyWorker::new("price-projector")),
            Arc::new(ReconciliationBackgroundWorker::new(
                Arc::clone(&store),
                authz,
                Duration::from_secs(1),
                100,
            )),
        ],
        Arc::new(RuntimeState::default()),
        shutdown,
    )
    .await?;
    Ok(())
}
