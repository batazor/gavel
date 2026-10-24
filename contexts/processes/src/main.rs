use std::{io, sync::Arc, time::Duration};

use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use gavel_processes::{
    activities::{OwnerTemporalActivities, SettlementTemporalActivities},
    adapters::{
        owner_rpc::OwnerRpcActivities,
        redpanda::{RedpandaBroker, RedpandaConfig},
    },
    bootstrap::{RuntimeConfig, RuntimeHealth},
    ingress::{
        ConsumerLoop, RetryPolicy as BrokerRetryPolicy, TemporalGateway, VersionedSignalIngress,
    },
    telemetry::{WorkerReadiness, WorkflowCompletionMetrics},
    workflows::{AuctionLifecycleWorkflow, SettleAuctionWorkflow},
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use temporalio_client::{
    Client, ClientOptions, Connection, ConnectionOptions,
    grpc::{HealthService, WorkflowService},
    tonic::IntoRequest,
};
use temporalio_common::protos::{
    grpc::health::v1::{HealthCheckRequest, health_check_response::ServingStatus},
    temporal::api::workflowservice::v1::DescribeNamespaceRequest,
};
use temporalio_sdk::{Runtime, Worker, WorkerOptions};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct HealthState {
    runtime: Arc<RuntimeHealth>,
    metrics: PrometheusHandle,
}

enum RuntimeExit {
    Lifecycle(Result<(), temporalio_sdk::WorkerRunError>),
    Settlement(Result<(), temporalio_sdk::WorkerRunError>),
    Consumer(Result<(), gavel_processes::ingress::DeliveryError>),
    Signal(Result<(), io::Error>),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gavel_processes=info,temporalio=info".into()),
        )
        .json()
        .init();
    let config = RuntimeConfig::from_env()?;
    let metrics = PrometheusBuilder::new().install_recorder()?;
    let runtime_health = Arc::new(RuntimeHealth::default());
    let health = HealthState {
        runtime: runtime_health.clone(),
        metrics,
    };
    let shutdown = CancellationToken::new();
    let http = start_http(config.http_addr, health.clone(), shutdown.clone()).await?;

    let runtime = Runtime::from_current_tokio(Default::default())?;
    let temporal_url = url::Url::parse(&config.temporal_endpoint)?;
    let connection_options = ConnectionOptions::new(temporal_url)
        .identity("gavel-processes")
        .connect_timeout(Duration::from_secs(5))
        .maybe_api_key(config.temporal_api_key.clone());
    let connection = Connection::connect(connection_options.build()).await?;
    let client = Client::new(
        connection,
        ClientOptions::new(config.temporal_namespace.clone()).build(),
    )?;
    validate_temporal_namespace(&client, &config.temporal_namespace).await?;
    if !temporal_is_serving(&client).await {
        return Err(io::Error::other("Temporal health service is not serving").into());
    }
    runtime_health.set_temporal_connected(true);
    let temporal_monitor = tokio::spawn(monitor_temporal(
        client.clone(),
        runtime_health.clone(),
        shutdown.clone(),
    ));

    let auction_client = gavel_auction::rpc::auction_service_client::AuctionServiceClient::connect(
        config.auction_endpoint.clone(),
    )
    .await?;
    let payments_client = gavel_payments_tonic::gavel::payments::v1::tonic::payments_service_client::PaymentsServiceClient::connect(
        config.payments_endpoint.clone(),
    )
    .await?;
    let notify_client = gavel_notify::rpc::notify_service_client::NotifyServiceClient::connect(
        config.notify_endpoint.clone(),
    )
    .await?;
    runtime_health.set_auction_rpc_ready(true);
    runtime_health.set_payments_rpc_ready(true);
    runtime_health.set_notify_rpc_ready(true);
    let owner_monitor = tokio::spawn(monitor_owner_rpcs(
        config.auction_endpoint.clone(),
        config.payments_endpoint.clone(),
        config.notify_endpoint.clone(),
        runtime_health.clone(),
        shutdown.clone(),
    ));
    let owners = Arc::new(OwnerRpcActivities::new_authenticated(
        auction_client,
        payments_client,
        notify_client,
        config.auction_principal_id,
        config.workload_spiffe_id,
    ));
    let broker = RedpandaBroker::connect(&RedpandaConfig {
        brokers: config.redpanda_brokers.clone(),
        group_id: config.redpanda_group_id.clone(),
        client_id: config.redpanda_client_id.clone(),
        security_protocol: config.redpanda_security_protocol.clone(),
        sasl_username: config.redpanda_sasl_username.clone(),
        sasl_password: config.redpanda_sasl_password.clone(),
        quarantine_timeout: config.quarantine_timeout,
    })?;
    runtime_health.set_broker_connected(broker.is_ready(Duration::from_secs(2)));
    let broker_monitor = tokio::spawn(monitor_broker(
        broker.clone(),
        runtime_health.clone(),
        shutdown.clone(),
    ));
    let signal_ingress = Arc::new(VersionedSignalIngress::new(TemporalGateway::new(
        client.clone(),
        config.lifecycle_task_queue.clone(),
        config.settlement_task_queue.clone(),
    )));
    let consumer = ConsumerLoop::new(
        broker,
        signal_ingress,
        BrokerRetryPolicy {
            max_attempts: config.broker_retry_attempts,
            initial_backoff: config.broker_retry_initial,
            max_backoff: config.broker_retry_max,
        },
    )?;

    let lifecycle_options = WorkerOptions::new(config.lifecycle_task_queue.clone())
        .register_workflow::<AuctionLifecycleWorkflow>()?
        .register_activities(OwnerTemporalActivities::new(owners.clone()))
        .worker_interceptor(WorkflowCompletionMetrics::new("auction_lifecycle"))
        .worker_interceptor(WorkerReadiness::new(runtime_health.clone()))
        .graceful_shutdown_period(config.graceful_shutdown)
        .build();
    let settlement_options = WorkerOptions::new(config.settlement_task_queue.clone())
        .register_workflow::<SettleAuctionWorkflow>()?
        .register_activities(SettlementTemporalActivities::new(
            owners.clone(),
            owners.clone(),
            owners,
        ))
        .worker_interceptor(WorkflowCompletionMetrics::new("settle_auction"))
        .worker_interceptor(WorkerReadiness::new(runtime_health.clone()))
        .graceful_shutdown_period(config.graceful_shutdown)
        .build();
    let mut lifecycle_worker = Worker::new(&runtime, client.clone(), lifecycle_options)?;
    let mut settlement_worker = Worker::new(&runtime, client, settlement_options)?;
    let stop_lifecycle = lifecycle_worker.shutdown_handle();
    let stop_settlement = settlement_worker.shutdown_handle();

    tracing::info!(
        namespace = %config.temporal_namespace,
        lifecycle_task_queue = %config.lifecycle_task_queue,
        settlement_task_queue = %config.settlement_task_queue,
        "process worker запущен"
    );

    let lifecycle_run = lifecycle_worker.run();
    let settlement_run = settlement_worker.run();
    let consumer_health = runtime_health.clone();
    let consumer_shutdown = shutdown.clone();
    let consumer_run = async move {
        let _active = consumer_health.consumer_started();
        consumer.run(consumer_shutdown).await
    };
    tokio::pin!(lifecycle_run, settlement_run, consumer_run);
    let exit = tokio::select! {
        result = &mut lifecycle_run => RuntimeExit::Lifecycle(result),
        result = &mut settlement_run => RuntimeExit::Settlement(result),
        result = &mut consumer_run => RuntimeExit::Consumer(result),
        signal = tokio::signal::ctrl_c() => RuntimeExit::Signal(signal),
    };

    runtime_health.begin_shutdown();
    stop_lifecycle();
    stop_settlement();
    shutdown.cancel();

    let workers_drained = tokio::time::timeout(config.graceful_shutdown, async {
        match &exit {
            RuntimeExit::Lifecycle(_) => {
                let _ = tokio::join!(settlement_run, consumer_run);
            }
            RuntimeExit::Settlement(_) => {
                let _ = tokio::join!(lifecycle_run, consumer_run);
            }
            RuntimeExit::Consumer(_) => {
                let _ = tokio::join!(lifecycle_run, settlement_run);
            }
            RuntimeExit::Signal(_) => {
                let _ = tokio::join!(lifecycle_run, settlement_run, consumer_run);
            }
        }
    })
    .await
    .is_ok();
    let http_drained = tokio::time::timeout(config.graceful_shutdown, http)
        .await
        .is_ok();
    let _ = tokio::join!(temporal_monitor, broker_monitor, owner_monitor);
    runtime_health.mark_stopped();

    if !workers_drained || !http_drained {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "graceful drain timed out").into());
    }
    match exit {
        RuntimeExit::Lifecycle(Ok(())) => {
            Err(io::Error::other("lifecycle worker stopped unexpectedly").into())
        }
        RuntimeExit::Lifecycle(Err(error)) => {
            Err(io::Error::other(format!("lifecycle worker stopped: {error}")).into())
        }
        RuntimeExit::Settlement(Ok(())) => {
            Err(io::Error::other("settlement worker stopped unexpectedly").into())
        }
        RuntimeExit::Settlement(Err(error)) => {
            Err(io::Error::other(format!("settlement worker stopped: {error}")).into())
        }
        RuntimeExit::Consumer(Ok(())) => {
            Err(io::Error::other("Redpanda consumer stopped unexpectedly").into())
        }
        RuntimeExit::Consumer(Err(error)) => {
            Err(io::Error::other(format!("Redpanda consumer stopped: {error}")).into())
        }
        RuntimeExit::Signal(result) => result.map_err(Into::into),
    }
}

async fn validate_temporal_namespace(
    client: &Client,
    namespace: &str,
) -> Result<(), temporalio_client::tonic::Status> {
    let mut service = client.clone();
    WorkflowService::describe_namespace(
        &mut service,
        DescribeNamespaceRequest {
            namespace: namespace.to_owned(),
            ..Default::default()
        }
        .into_request(),
    )
    .await?;
    Ok(())
}

async fn temporal_is_serving(client: &Client) -> bool {
    let mut service = client.clone();
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        HealthService::check(&mut service, HealthCheckRequest::default().into_request()),
    )
    .await;
    matches!(
        response,
        Ok(Ok(response)) if response.get_ref().status == ServingStatus::Serving as i32
    )
}

async fn monitor_temporal(client: Client, health: Arc<RuntimeHealth>, shutdown: CancellationToken) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = interval.tick() => {
                health.set_temporal_connected(temporal_is_serving(&client).await);
            }
        }
    }
    health.set_temporal_connected(false);
}

async fn monitor_broker(
    broker: Arc<RedpandaBroker>,
    health: Arc<RuntimeHealth>,
    shutdown: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = interval.tick() => {
                let broker = broker.clone();
                let ready = tokio::task::spawn_blocking(move || {
                    broker.is_ready(Duration::from_secs(1))
                })
                .await
                .unwrap_or(false);
                health.set_broker_connected(ready);
            }
        }
    }
    health.set_broker_connected(false);
}

async fn monitor_owner_rpcs(
    auction_endpoint: String,
    payments_endpoint: String,
    notify_endpoint: String,
    health: Arc<RuntimeHealth>,
    shutdown: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = interval.tick() => {
                health.set_auction_rpc_ready(
                    rpc_endpoint_is_ready(&auction_endpoint).await
                );
                health.set_payments_rpc_ready(
                    rpc_endpoint_is_ready(&payments_endpoint).await
                );
                health.set_notify_rpc_ready(
                    rpc_endpoint_is_ready(&notify_endpoint).await
                );
            }
        }
    }
    health.set_auction_rpc_ready(false);
    health.set_payments_rpc_ready(false);
    health.set_notify_rpc_ready(false);
}

async fn rpc_endpoint_is_ready(endpoint: &str) -> bool {
    let Ok(endpoint) = tonic::transport::Endpoint::from_shared(endpoint.to_owned()) else {
        return false;
    };
    matches!(
        tokio::time::timeout(
            Duration::from_secs(1),
            endpoint.connect_timeout(Duration::from_secs(1)).connect(),
        )
        .await,
        Ok(Ok(_))
    )
}

async fn start_http(
    address: std::net::SocketAddr,
    health: HealthState,
    shutdown: CancellationToken,
) -> Result<tokio::task::JoinHandle<()>, std::io::Error> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    let router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .with_state(health);
    Ok(tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
        {
            tracing::error!(%error, "HTTP exporter остановлен");
        }
    }))
}

async fn healthz(State(state): State<HealthState>) -> impl IntoResponse {
    if state.runtime.is_live() {
        (StatusCode::OK, "{\"status\":\"ok\"}\n")
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "{\"status\":\"stopping\"}\n",
        )
    }
}

async fn readyz(State(state): State<HealthState>) -> impl IntoResponse {
    let snapshot = state.runtime.readiness_snapshot();
    let body = serde_json::to_string(&snapshot).unwrap_or_else(|_| "{\"ready\":false}".to_owned());
    if snapshot.ready {
        (StatusCode::OK, body)
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, body)
    }
}

async fn metrics(State(state): State<HealthState>) -> String {
    state.metrics.render()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health_state() -> HealthState {
        let recorder = PrometheusBuilder::new().build_recorder();
        HealthState {
            runtime: Arc::new(RuntimeHealth::default()),
            metrics: recorder.handle(),
        }
    }

    #[tokio::test]
    async fn probes_follow_poller_readiness_and_shutdown_state() {
        let state = health_state();
        assert_eq!(
            healthz(State(state.clone())).await.into_response().status(),
            StatusCode::OK
        );
        assert_eq!(
            readyz(State(state.clone())).await.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );

        state.runtime.set_temporal_connected(true);
        state.runtime.set_broker_connected(true);
        state.runtime.set_auction_rpc_ready(true);
        state.runtime.set_payments_rpc_ready(true);
        state.runtime.set_notify_rpc_ready(true);
        let consumer = state.runtime.consumer_started();
        let lifecycle = state.runtime.worker_started();
        let settlement = state.runtime.worker_started();
        let response = readyz(State(state.clone())).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        state.runtime.begin_shutdown();
        assert_eq!(
            readyz(State(state.clone())).await.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        drop((lifecycle, settlement, consumer));
        state.runtime.mark_stopped();
        assert_eq!(
            healthz(State(state)).await.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn http_exporter_stops_within_graceful_drain_bound() {
        let shutdown = CancellationToken::new();
        let server = start_http(
            "127.0.0.1:0".parse().unwrap(),
            health_state(),
            shutdown.clone(),
        )
        .await
        .unwrap();

        shutdown.cancel();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), server)
                .await
                .is_ok()
        );
    }
}
