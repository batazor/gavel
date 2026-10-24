#![allow(clippy::result_large_err)]

use std::{io, sync::Arc, time::Duration};

use axum::{Router, routing::get};
use gavel_notify::{
    adapters::{
        inbound::{
            GrpcNotify, HttpState, authenticate_operator, authenticate_workload, health,
            local_notification, metrics, ready,
        },
        outbound::{PostgresNotifications, SmtpEmailGateway},
    },
    application::{NotifyApplication, RetryPolicy},
    bootstrap::Config,
    rpc::{
        notify_operator_service_server::NotifyOperatorServiceServer,
        notify_service_server::NotifyServiceServer,
    },
};
use metrics_exporter_prometheus::PrometheusBuilder;
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        let url = std::env::var("DATABASE_URL")
            .map_err(|_| io::Error::other("required configuration is missing"))?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await?;
        PostgresNotifications::new(pool).migrate().await?;
        return Ok(());
    }
    let config = Config::from_env()?;
    let metrics_handle = PrometheusBuilder::new().install_recorder()?;
    metrics::gauge!("gavel_notify_service_info").set(1.0);
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await?;
    let repository = Arc::new(PostgresNotifications::new(pool));
    let smtp = Arc::new(
        SmtpEmailGateway::new(config.smtp_addr, config.smtp_from, config.smtp_timeout)
            .map_err(io::Error::other)?,
    );
    let retry = RetryPolicy::new(
        config.delivery_attempts,
        config.retry_base,
        config.retry_max,
    )?;
    let application = Arc::new(NotifyApplication::new(
        repository.clone(),
        smtp.clone(),
        retry,
    ));
    let http = Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .route("/local/operator/notification", get(local_notification))
        .with_state(HttpState {
            repository,
            smtp,
            metrics: metrics_handle,
            application: application.clone(),
            local_operator_token: config.local_operator_token.clone(),
        });

    let grpc = GrpcNotify::new(
        application.clone(),
        config.processes_spiffe_id.clone(),
        config.operator_principal.clone(),
        Duration::from_secs(15),
    );
    let operator_principal = config.operator_principal;
    let send = NotifyServiceServer::with_interceptor(grpc, authenticate_workload);
    // The operator service is served by a separate instance so each interceptor installs only
    // the identity authenticated by the mesh authorization policy for that service path.
    let operator = GrpcNotify::new(
        application,
        String::new(),
        operator_principal.clone(),
        Duration::from_secs(15),
    );
    let operator = NotifyOperatorServiceServer::with_interceptor(operator, authenticate_operator);

    let http_listener = tokio::net::TcpListener::bind(config.http_addr).await?;
    let shutdown = CancellationToken::new();
    let mut tasks = JoinSet::new();
    let http_shutdown = shutdown.clone();
    tasks.spawn(async move {
        axum::serve(http_listener, http)
            .with_graceful_shutdown(http_shutdown.cancelled_owned())
            .await
            .map_err(|error| format!("HTTP server stopped: {error}"))
    });
    let grpc_shutdown = shutdown.clone();
    tasks.spawn(async move {
        Server::builder()
            .add_service(send)
            .add_service(operator)
            .serve_with_shutdown(config.grpc_addr, grpc_shutdown.cancelled_owned())
            .await
            .map_err(|error| format!("gRPC server stopped: {error}"))
    });
    tracing::info!(http_addr = %config.http_addr, grpc_addr = %config.grpc_addr, "notify started");
    let failure = tokio::select! {
        result = tasks.join_next() => Some(match result {
            Some(Ok(Ok(()))) => "supervised runtime task stopped unexpectedly".to_owned(),
            Some(Ok(Err(error))) => error,
            Some(Err(error)) => format!("supervised runtime task failed: {error}"),
            None => "all supervised runtime tasks stopped unexpectedly".to_owned(),
        }),
        result = tokio::signal::ctrl_c() => { result?; None },
    };
    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    if let Some(failure) = failure {
        return Err(io::Error::other(failure).into());
    }
    Ok(())
}
