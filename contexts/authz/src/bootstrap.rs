use std::{
    env,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use tokio::{net::TcpListener, time::MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::{adapters::inbound::TrustedProxyCidr, telemetry::render_permission_prometheus};

#[derive(Clone, Debug)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub grpc_addr: SocketAddr,
    pub database_url: String,
    pub spicedb_endpoint: String,
    pub spicedb_credential: String,
    pub principal_key: [u8; 32],
    pub fingerprint_key: [u8; 32],
    pub dependency_timeout: Duration,
    pub receipt_lease: Duration,
    pub worker_interval: Duration,
    pub spiffe_prefix: String,
    pub trusted_proxies: Vec<TrustedProxyCidr>,
    pub test_identity: Option<crate::application::WorkloadIdentity>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("required environment variable {0} is missing")]
    Missing(&'static str),
    #[error("environment variable {0} is invalid")]
    Invalid(&'static str),
    #[error("test workload identity requires AUTHZ_ENABLE_TEST_IDENTITY=true")]
    TestIdentityDisabled,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let test_identity_raw = env::var("AUTHZ_TEST_IDENTITY").ok();
        let test_identity_enabled = env::var("AUTHZ_ENABLE_TEST_IDENTITY").as_deref() == Ok("true");
        if test_identity_raw.is_some() && !test_identity_enabled {
            return Err(ConfigError::TestIdentityDisabled);
        }
        Ok(Self {
            http_addr: parse("AUTHZ_HTTP_ADDR")?,
            grpc_addr: parse("AUTHZ_GRPC_ADDR")?,
            database_url: required("AUTHZ_DATABASE_URL")?,
            spicedb_endpoint: required("AUTHZ_SPICEDB_ENDPOINT")?,
            spicedb_credential: required("AUTHZ_SPICEDB_CREDENTIAL")?,
            principal_key: key("AUTHZ_PRINCIPAL_HMAC_KEY")?,
            fingerprint_key: key("AUTHZ_FINGERPRINT_HMAC_KEY")?,
            dependency_timeout: millis("AUTHZ_DEPENDENCY_TIMEOUT_MS")?,
            receipt_lease: millis("AUTHZ_RECEIPT_LEASE_MS")?,
            worker_interval: millis("AUTHZ_WORKER_INTERVAL_MS")?,
            spiffe_prefix: required("AUTHZ_SPIFFE_PREFIX")?,
            trusted_proxies: env::var("AUTHZ_TRUSTED_PROXY_CIDRS")
                .unwrap_or_default()
                .split(',')
                .filter(|value| !value.trim().is_empty())
                .map(|value| {
                    value
                        .trim()
                        .parse()
                        .map_err(|_| ConfigError::Invalid("AUTHZ_TRUSTED_PROXY_CIDRS"))
                })
                .collect::<Result<_, _>>()?,
            test_identity: test_identity_raw
                .map(|value| parse_workload(&value))
                .transpose()?,
        })
    }
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(ConfigError::Missing(name))
}

fn parse<T: std::str::FromStr>(name: &'static str) -> Result<T, ConfigError> {
    required(name)?
        .parse()
        .map_err(|_| ConfigError::Invalid(name))
}

fn millis(name: &'static str) -> Result<Duration, ConfigError> {
    let value: u64 = parse(name)?;
    if value == 0 {
        Err(ConfigError::Invalid(name))
    } else {
        Ok(Duration::from_millis(value))
    }
}

fn key(name: &'static str) -> Result<[u8; 32], ConfigError> {
    let bytes = STANDARD
        .decode(required(name)?)
        .map_err(|_| ConfigError::Invalid(name))?;
    bytes.try_into().map_err(|_| ConfigError::Invalid(name))
}

fn parse_workload(value: &str) -> Result<crate::application::WorkloadIdentity, ConfigError> {
    use crate::application::WorkloadIdentity;
    match value {
        "bff" => Ok(WorkloadIdentity::Bff),
        "auction" => Ok(WorkloadIdentity::Auction),
        "catalog" => Ok(WorkloadIdentity::Catalog),
        "payments" => Ok(WorkloadIdentity::Payments),
        "notify" => Ok(WorkloadIdentity::Notify),
        "processes" => Ok(WorkloadIdentity::Processes),
        _ => Err(ConfigError::Invalid("AUTHZ_TEST_IDENTITY")),
    }
}

#[derive(Clone, Default)]
pub struct HealthState {
    live: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
}

impl HealthState {
    pub fn start() -> Self {
        Self {
            live: Arc::new(AtomicBool::new(true)),
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_ready(&self, value: bool) {
        self.ready.store(value, Ordering::Release);
    }

    pub fn stop(&self) {
        self.ready.store(false, Ordering::Release);
        self.live.store(false, Ordering::Release);
    }
}

pub fn http_router(state: HealthState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .with_state(state)
}

pub async fn serve_http(
    listener: TcpListener,
    state: HealthState,
    shutdown: CancellationToken,
) -> std::io::Result<()> {
    axum::serve(listener, http_router(state))
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
}

async fn health(State(state): State<HealthState>) -> Response {
    status_response(state.live.load(Ordering::Acquire))
}

async fn ready(State(state): State<HealthState>) -> Response {
    status_response(state.ready.load(Ordering::Acquire))
}

fn status_response(ok: bool) -> Response {
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({ "status": if ok { "ok" } else { "unavailable" } })),
    )
        .into_response()
}

async fn metrics() -> Response {
    (
        StatusCode::OK,
        format!(
            "# TYPE gavel_service_info gauge\ngavel_service_info{{service=\"authz\"}} 1\n{}",
            render_permission_prometheus()
        ),
    )
        .into_response()
}

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProbeError {
    #[error("dependency is unavailable")]
    Unavailable,
    #[error("readiness worker failed")]
    Fatal,
}

#[async_trait]
pub trait ReceiptWorkerProbe: Send + Sync {
    async fn probe(&self) -> Result<(), ProbeError>;
}

pub async fn run_receipt_worker<P: ReceiptWorkerProbe>(
    probe: &P,
    state: HealthState,
    interval: Duration,
    shutdown: CancellationToken,
) -> Result<(), ProbeError> {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                state.set_ready(false);
                return Ok(());
            }
            _ = ticker.tick() => match probe.probe().await {
                Ok(()) => state.set_ready(true),
                Err(ProbeError::Unavailable) => state.set_ready(false),
                Err(ProbeError::Fatal) => {
                    state.set_ready(false);
                    return Err(ProbeError::Fatal);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    async fn status(address: SocketAddr, path: &str) -> u16 {
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
        response.split_whitespace().nth(1).unwrap().parse().unwrap()
    }

    #[tokio::test]
    async fn real_health_endpoints_track_startup_readiness_and_shutdown() {
        let state = HealthState::start();
        let shutdown = CancellationToken::new();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(serve_http(listener, state.clone(), shutdown.clone()));

        assert_eq!(status(address, "/healthz").await, 200);
        assert_eq!(status(address, "/readyz").await, 503);
        state.set_ready(true);
        assert_eq!(status(address, "/readyz").await, 200);
        state.stop();
        assert_eq!(status(address, "/healthz").await, 503);
        shutdown.cancel();
        task.await.unwrap().unwrap();
    }

    struct FakeProbe(AtomicU8);

    #[async_trait]
    impl ReceiptWorkerProbe for FakeProbe {
        async fn probe(&self) -> Result<(), ProbeError> {
            match self.0.fetch_add(1, Ordering::SeqCst) {
                0 => Err(ProbeError::Unavailable),
                1 => Ok(()),
                _ => Err(ProbeError::Fatal),
            }
        }
    }

    #[tokio::test]
    async fn receipt_worker_recovers_readiness_and_surfaces_fatal_failure() {
        let state = HealthState::start();
        let error = run_receipt_worker(
            &FakeProbe(AtomicU8::new(0)),
            state.clone(),
            Duration::from_millis(1),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error, ProbeError::Fatal);
        assert!(!state.ready.load(Ordering::Acquire));
    }
}
