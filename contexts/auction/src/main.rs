use std::{sync::Arc, time::Duration};

use gavel_auction::{
    adapters::{
        inbound::{
            graphql::{AuctionGraphqlApi, AuctionGraphqlApplication, schema},
            grpc::{AuctionGrpcApi, AuctionGrpcApplication, AuctionGrpcService, SystemClock},
            redpanda::{
                AuctionOwnerEventHandler, OwnerEventWorker, RedpandaConfig, RedpandaOwnerBroker,
            },
        },
        outbound::{
            authz::AuthzGrpcAdapter, postgres::PostgresAuctionStore, redis::RedisPriceCache,
        },
    },
    bootstrap::{
        BackgroundWorker, ReconciliationBackgroundWorker, RuntimeConfig, RuntimeState,
        StandbyWorker, serve,
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
    let authz = Arc::new(
        AuthzGrpcAdapter::connect_authenticated(
            &config.authz_url,
            config.dependency_timeout,
            &std::env::var("AUTHZ_CALLER_SPIFFE_ID")?,
        )
        .await?,
    );
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
    let grpc_service = AuctionGrpcService::new_authenticated(
        grpc_api as Arc<dyn AuctionGrpcApi>,
        Arc::new(SystemClock),
        std::env::var("PROCESSES_SPIFFE_ID")?,
    );
    let owner_broker = RedpandaOwnerBroker::connect(&RedpandaConfig {
        brokers: config.redpanda_brokers.clone(),
        group_id: config.redpanda_group_id.clone(),
        client_id: config.redpanda_client_id.clone(),
        security_protocol: config.redpanda_security_protocol.clone(),
        sasl_username: config.redpanda_sasl_username.clone(),
        sasl_password: config.redpanda_sasl_password.clone(),
    })?;
    let owner_handler = Arc::new(AuctionOwnerEventHandler::new(
        Arc::clone(&store),
        3,
        config.fingerprint_key,
    ));
    let workers: Vec<Arc<dyn BackgroundWorker>> = vec![
        Arc::new(OwnerEventWorker::new(owner_broker, owner_handler)),
        Arc::new(StandbyWorker::new("price-projector")),
        Arc::new(ReconciliationBackgroundWorker::new(
            Arc::clone(&store),
            authz,
            Duration::from_secs(1),
            100,
        )),
    ];
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
        workers,
        Arc::new(RuntimeState::default()),
        shutdown,
    )
    .await?;
    Ok(())
}
