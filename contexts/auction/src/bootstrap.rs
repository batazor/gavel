//! Tokio runtime composition and process lifecycle.

use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use async_trait::async_trait;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use tokio::{net::TcpListener, task::JoinSet};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;

use crate::{
    adapters::inbound::{
        graphql::{AuctionSchema, VerifiedRequestContext},
        grpc::AuctionGrpcService,
    },
    application::{
        Authorization, Principal, PrincipalKind, ReconciliationQueue, ReconciliationWorker,
    },
    domain::Timestamp,
    rpc::auction_service_server::AuctionServiceServer,
};

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub http_addr: SocketAddr,
    pub grpc_addr: SocketAddr,
    pub database_url: String,
    pub redis_url: String,
    pub authz_url: String,
    pub fingerprint_key: [u8; 32],
    pub dependency_timeout: Duration,
}

impl RuntimeConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let timeout_millis = env::var("DEPENDENCY_TIMEOUT_MILLIS")
            .unwrap_or_else(|_| "500".to_owned())
            .parse::<u64>()
            .map_err(|_| ConfigError::Invalid("DEPENDENCY_TIMEOUT_MILLIS"))?;
        if timeout_millis == 0 {
            return Err(ConfigError::Invalid("DEPENDENCY_TIMEOUT_MILLIS"));
        }
        Ok(Self {
            http_addr: socket_addr("HTTP_ADDR", 8080)?,
            grpc_addr: socket_addr("GRPC_ADDR", 9090)?,
            database_url: required("DATABASE_URL")?,
            redis_url: required("REDIS_URL")?,
            authz_url: required("AUTHZ_URL")?,
            fingerprint_key: decode_key(&required("COMMAND_FINGERPRINT_KEY_HEX")?)?,
            dependency_timeout: Duration::from_millis(timeout_millis),
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("required environment variable {0} is missing")]
    Missing(&'static str),
    #[error("environment variable {0} is invalid")]
    Invalid(&'static str),
}

#[derive(Default)]
pub struct RuntimeState {
    ready: AtomicBool,
}

impl RuntimeState {
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

#[async_trait]
pub trait BackgroundWorker: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(&self, shutdown: CancellationToken) -> Result<(), String>;
}

pub struct StandbyWorker {
    name: &'static str,
}

impl StandbyWorker {
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }
}

#[async_trait]
impl BackgroundWorker for StandbyWorker {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn run(&self, shutdown: CancellationToken) -> Result<(), String> {
        shutdown.cancelled().await;
        Ok(())
    }
}

pub struct ReconciliationBackgroundWorker<Q, A> {
    worker: ReconciliationWorker<Q, A>,
    interval: Duration,
    batch_size: usize,
}

impl<Q, A> ReconciliationBackgroundWorker<Q, A>
where
    Q: ReconciliationQueue,
    A: Authorization,
{
    pub fn new(
        queue: Arc<Q>,
        authorization: Arc<A>,
        interval: Duration,
        batch_size: usize,
    ) -> Self {
        Self {
            worker: ReconciliationWorker::new(queue, authorization),
            interval,
            batch_size: batch_size.max(1),
        }
    }
}

#[async_trait]
impl<Q, A> BackgroundWorker for ReconciliationBackgroundWorker<Q, A>
where
    Q: ReconciliationQueue + 'static,
    A: Authorization + 'static,
{
    fn name(&self) -> &'static str {
        "authz-reconciliation"
    }

    async fn run(&self, shutdown: CancellationToken) -> Result<(), String> {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = tokio::time::sleep(self.interval) => {
                    self.worker
                        .run_batch(self.batch_size)
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
        }
    }
}

#[derive(Clone)]
struct HttpState {
    schema: AuctionSchema,
    runtime: Arc<RuntimeState>,
}

pub fn http_router(schema: AuctionSchema, runtime: Arc<RuntimeState>) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .route("/graphql", post(graphql))
        .with_state(HttpState { schema, runtime })
}

pub async fn serve(
    http_listener: TcpListener,
    grpc_listener: TcpListener,
    schema: AuctionSchema,
    grpc_service: AuctionGrpcService,
    workers: Vec<Arc<dyn BackgroundWorker>>,
    runtime: Arc<RuntimeState>,
    shutdown: CancellationToken,
) -> Result<(), RuntimeError> {
    let app = http_router(schema, Arc::clone(&runtime));
    let server_shutdown = shutdown.child_token();
    let mut tasks = JoinSet::new();
    let http_shutdown = server_shutdown.clone();
    tasks.spawn(async move {
        axum::serve(http_listener, app)
            .with_graceful_shutdown(http_shutdown.cancelled_owned())
            .await
            .map_err(|error| error.to_string())
    });
    let grpc_shutdown = server_shutdown.clone();
    tasks.spawn(async move {
        Server::builder()
            .add_service(AuctionServiceServer::new(grpc_service))
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
    runtime.ready.store(true, Ordering::Release);

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
    runtime.ready.store(false, Ordering::Release);
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
    axum::Json(HealthBody {
        status: "ok",
        service: "auction",
    })
}

async fn ready(State(state): State<HttpState>) -> Response {
    let (status, body_status) = if state.runtime.is_ready() {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not_ready")
    };
    (
        status,
        axum::Json(HealthBody {
            status: body_status,
            service: "auction",
        }),
    )
        .into_response()
}

async fn metrics() -> impl IntoResponse {
    (StatusCode::OK, crate::telemetry::render_prometheus())
}

async fn graphql(
    State(state): State<HttpState>,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> GraphQLResponse {
    let mut request = request.into_inner();
    if let Some(verified) = verified_http_context(&headers) {
        request = request.data(verified);
    }
    state.schema.execute(request).await.into()
}

fn verified_http_context(headers: &HeaderMap) -> Option<VerifiedRequestContext> {
    let principal_id = headers.get("x-gavel-user-id")?.to_str().ok()?.trim();
    let correlation_id = headers.get("x-correlation-id")?.to_str().ok()?.trim();
    if principal_id.is_empty() || correlation_id.is_empty() {
        return None;
    }
    let causation_id = headers
        .get("x-causation-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(correlation_id);
    Some(VerifiedRequestContext {
        principal: Principal {
            kind: PrincipalKind::User,
            id: principal_id.to_owned(),
        },
        correlation_id: correlation_id.to_owned(),
        causation_id: causation_id.to_owned(),
        now: system_timestamp(),
    })
}

fn system_timestamp() -> Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    Timestamp::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::Missing(name))
}

fn socket_addr(name: &'static str, default_port: u16) -> Result<SocketAddr, ConfigError> {
    env::var(name)
        .unwrap_or_else(|_| {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), default_port).to_string()
        })
        .parse()
        .map_err(|_| ConfigError::Invalid(name))
}

fn decode_key(value: &str) -> Result<[u8; 32], ConfigError> {
    if value.len() != 64 {
        return Err(ConfigError::Invalid("COMMAND_FINGERPRINT_KEY_HEX"));
    }
    let mut key = [0_u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| ConfigError::Invalid("COMMAND_FINGERPRINT_KEY_HEX"))?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::inbound::{
            graphql::{AuctionGraphqlApi, schema},
            grpc::{AuctionGrpcApi, Clock},
        },
        application::{
            ApplicationError, AuctionView, CancelAuctionInput, CloseAuctionInput, CommandContext,
            ConfirmWinnerInput, ForfeitSettlementCandidateInput, LifecycleResult, PlaceBidInput,
            PlaceBidResult, RescheduleAuctionInput,
        },
    };

    struct EmptyApi;

    #[async_trait]
    impl AuctionGraphqlApi for EmptyApi {
        async fn auction(&self, _: &str) -> Result<Option<AuctionView>, ApplicationError> {
            Ok(None)
        }
        async fn place_bid(
            &self,
            _: &CommandContext,
            _: PlaceBidInput,
        ) -> Result<PlaceBidResult, ApplicationError> {
            Err(ApplicationError::Internal)
        }
        async fn reschedule_auction(
            &self,
            _: &CommandContext,
            _: RescheduleAuctionInput,
        ) -> Result<AuctionView, ApplicationError> {
            Err(ApplicationError::Internal)
        }
    }

    #[async_trait]
    impl AuctionGrpcApi for EmptyApi {
        async fn close(
            &self,
            _: &CommandContext,
            _: CloseAuctionInput,
        ) -> Result<LifecycleResult, ApplicationError> {
            Err(ApplicationError::Internal)
        }
        async fn cancel(
            &self,
            _: &CommandContext,
            _: CancelAuctionInput,
        ) -> Result<LifecycleResult, ApplicationError> {
            Err(ApplicationError::Internal)
        }
        async fn forfeit(
            &self,
            _: &CommandContext,
            _: ForfeitSettlementCandidateInput,
        ) -> Result<LifecycleResult, ApplicationError> {
            Err(ApplicationError::Internal)
        }
        async fn confirm(
            &self,
            _: &CommandContext,
            _: ConfirmWinnerInput,
        ) -> Result<LifecycleResult, ApplicationError> {
            Err(ApplicationError::Internal)
        }
    }

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            Timestamp::from_millis(1)
        }
    }

    struct FailingWorker;

    #[async_trait]
    impl BackgroundWorker for FailingWorker {
        fn name(&self) -> &'static str {
            "failing-worker"
        }
        async fn run(&self, _: CancellationToken) -> Result<(), String> {
            Err("boom".to_owned())
        }
    }

    #[tokio::test]
    async fn real_health_and_readiness_follow_runtime_lifecycle() {
        let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = http.local_addr().unwrap();
        let grpc = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = Arc::new(EmptyApi);
        let runtime = Arc::new(RuntimeState::default());
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(serve(
            http,
            grpc,
            schema(Arc::clone(&api) as Arc<dyn AuctionGraphqlApi>),
            AuctionGrpcService::new(api as Arc<dyn AuctionGrpcApi>, Arc::new(FixedClock)),
            vec![Arc::new(StandbyWorker::new("test-worker"))],
            Arc::clone(&runtime),
            shutdown.clone(),
        ));
        for _ in 0..20 {
            if runtime.is_ready() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(runtime.is_ready());
        assert!(
            http_get(http_addr, "/healthz")
                .await
                .starts_with("HTTP/1.1 200")
        );
        assert!(
            http_get(http_addr, "/readyz")
                .await
                .starts_with("HTTP/1.1 200")
        );
        shutdown.cancel();
        task.await.unwrap().unwrap();
        assert!(!runtime.is_ready());
    }

    #[tokio::test]
    async fn worker_failure_makes_the_runtime_unready() {
        let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let grpc = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = Arc::new(EmptyApi);
        let runtime = Arc::new(RuntimeState::default());
        let result = serve(
            http,
            grpc,
            schema(Arc::clone(&api) as Arc<dyn AuctionGraphqlApi>),
            AuctionGrpcService::new(api as Arc<dyn AuctionGrpcApi>, Arc::new(FixedClock)),
            vec![Arc::new(FailingWorker)],
            Arc::clone(&runtime),
            CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(result, Err(RuntimeError::ComponentFailed(message)) if message.contains("failing-worker"))
        );
        assert!(!runtime.is_ready());
    }

    #[test]
    fn fingerprint_key_validation_rejects_missing_or_non_hex_material() {
        assert_eq!(
            decode_key("short"),
            Err(ConfigError::Invalid("COMMAND_FINGERPRINT_KEY_HEX"))
        );
        assert_eq!(
            decode_key(&"zz".repeat(32)),
            Err(ConfigError::Invalid("COMMAND_FINGERPRINT_KEY_HEX"))
        );
        assert_eq!(decode_key(&"01".repeat(32)).unwrap(), [1; 32]);
    }

    async fn http_get(address: SocketAddr, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }
}
