use std::{io, sync::Arc, time::Duration};

use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use gavel_payments::{
    adapters::{
        inbound::{
            AuthenticatedPrincipal, CallbackIngress, GrpcPayments, PaymentSchema,
            StripeCallbackSink, authenticate_workload, build_schema, callback_router,
            sandbox_control_router,
        },
        outbound::{
            AuthzGrpc, MetricsTelemetry, PostgresPayments, SandboxGateway, StripeConfig,
            StripeGateway, StripeSignatureVerifier, SystemClock, UuidIds,
        },
    },
    application::{CommandFingerprint, PaymentApplication, ProviderGateway, ReceiptExecutor},
    bootstrap::{Config, ProviderConfig},
    rpc::payments_service_server::PaymentsServiceServer,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tonic_bsr::transport::Server;

type ProviderRuntime = (
    Arc<dyn ProviderGateway>,
    Option<StripeConfig>,
    Option<(Arc<SandboxGateway>, String)>,
);

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| io::Error::other("required configuration is missing"))?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await?;
        PostgresPayments::new(pool).migrate().await?;
        return Ok(());
    }
    let config = Config::from_env()?;
    let metrics_handle = PrometheusBuilder::new().install_recorder()?;
    metrics::gauge!("gavel_payments_service_info").set(1.0);
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await?;
    let postgres = Arc::new(PostgresPayments::new(pool));
    let authz = Arc::new(
        AuthzGrpc::connect_authenticated(
            &config.authz_endpoint,
            &std::env::var("AUTHZ_CALLER_SPIFFE_ID")?,
        )
        .await
        .map_err(|_| io::Error::other("authz dependency unavailable"))?,
    );
    let (provider, stripe_config, sandbox_runtime): ProviderRuntime = match config.provider.clone()
    {
        ProviderConfig::Stripe {
            endpoint,
            secret,
            callback_secret,
        } => {
            let provider_config = StripeConfig::new(endpoint, secret, callback_secret, 300)
                .map_err(|_| io::Error::other("provider configuration invalid"))?;
            (
                Arc::new(StripeGateway::new(
                    reqwest::Client::new(),
                    provider_config.clone(),
                )),
                Some(provider_config),
                None,
            )
        }
        ProviderConfig::Sandbox {
            default_outcome,
            control_token,
        } => {
            let gateway = Arc::new(SandboxGateway::new(default_outcome));
            (gateway.clone(), None, Some((gateway, control_token)))
        }
    };
    let clock = Arc::new(SystemClock);
    let receipts = Arc::new(ReceiptExecutor::new(
        postgres.clone(),
        clock.clone(),
        CommandFingerprint::new(config.fingerprint_secret.as_bytes())?,
        30_000,
    ));
    let application = Arc::new(PaymentApplication {
        uow: postgres.clone(),
        provider,
        authz,
        clock,
        ids: Arc::new(UuidIds),
        receipts,
        telemetry: Arc::new(MetricsTelemetry),
        callbacks: postgres.clone(),
    });

    let state = HttpState {
        schema: build_schema(application.clone()),
        postgres: postgres.clone(),
        metrics: metrics_handle,
    };
    let mut http = Router::new()
        .route("/graphql", post(graphql))
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .with_state(state);
    if let Some(provider_config) = stripe_config {
        let callback_sink = Arc::new(StripeCallbackSink::new(
            application.clone(),
            CommandFingerprint::new(config.fingerprint_secret.as_bytes())?,
        ));
        let callback_ingress = Arc::new(CallbackIngress {
            verifier: StripeSignatureVerifier::new(&provider_config),
            sink: callback_sink,
            now_secs: Arc::new(|| {
                u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default()
            }),
        });
        http = http.merge(callback_router(callback_ingress));
    }
    if let Some((gateway, control_token)) = sandbox_runtime {
        http = http.merge(sandbox_control_router(
            application.clone(),
            gateway,
            control_token,
        ));
    }

    let grpc = GrpcPayments::new(
        application.clone(),
        config.processes_spiffe_id.clone(),
        Duration::from_secs(15),
    );
    let grpc_service = PaymentsServiceServer::with_interceptor(grpc, authenticate_workload);
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
            .add_service(grpc_service)
            .serve_with_shutdown(config.grpc_addr, grpc_shutdown.cancelled_owned())
            .await
            .map_err(|error| format!("gRPC server stopped: {error}"))
    });
    let worker_shutdown = shutdown.clone();
    tasks.spawn(async move {
        run_reconciliation(application, postgres, worker_shutdown).await;
        Ok(())
    });
    tracing::info!(http_addr = %config.http_addr, grpc_addr = %config.grpc_addr, "payments started");
    let failure = tokio::select! {
        result = tasks.join_next() => Some(match result {
            Some(Ok(Ok(()))) => "supervised runtime task stopped unexpectedly".to_owned(),
            Some(Ok(Err(error))) => error,
            Some(Err(error)) => format!("supervised runtime task failed: {error}"),
            None => "all supervised runtime tasks stopped unexpectedly".to_owned(),
        }),
        result = tokio::signal::ctrl_c() => {
            result?;
            tracing::info!("shutdown requested");
            None
        },
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

async fn run_reconciliation(
    application: Arc<PaymentApplication>,
    postgres: Arc<PostgresPayments>,
    shutdown: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = interval.tick() => {}
        }
        let before = u64::try_from(chrono::Utc::now().timestamp_millis())
            .unwrap_or_default()
            .saturating_sub(120_000);
        match application.reconcile_stale_attempts(before, 100).await {
            Ok(count) => metrics::gauge!("gavel_payments_reconciled_attempts").set(count as f64),
            Err(error) => {
                tracing::warn!(error_code = ?error.code(), "payment reconciliation deferred")
            }
        }
        if let Ok(age) = postgres.outbox_backlog_age_seconds().await {
            metrics::gauge!("gavel_payments_outbox_backlog_age_seconds").set(age);
        }
        if let Ok(cleaned) = application.reconcile_orphan_grants(100).await {
            metrics::gauge!("gavel_payments_orphan_grants_cleaned").set(cleaned as f64);
        }
    }
}

#[derive(Clone)]
struct HttpState {
    schema: PaymentSchema,
    postgres: Arc<PostgresPayments>,
    metrics: PrometheusHandle,
}
async fn graphql(
    State(state): State<HttpState>,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> GraphQLResponse {
    let mut request = request.into_inner();
    if let Some(principal) = headers
        .get("x-gavel-principal")
        .and_then(|value| value.to_str().ok())
    {
        request = request.data(AuthenticatedPrincipal(principal.to_owned()));
    }
    state.schema.execute(request).await.into()
}
async fn health() -> StatusCode {
    StatusCode::OK
}
async fn ready(State(state): State<HttpState>) -> StatusCode {
    if sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(state.postgres.pool())
        .await
        .is_ok()
    {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
async fn metrics(State(state): State<HttpState>) -> impl IntoResponse {
    (StatusCode::OK, state.metrics.render())
}
