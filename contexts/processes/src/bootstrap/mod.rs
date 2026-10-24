//! Runtime composition boundary.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
    time::Duration,
};

const EXPECTED_WORKERS: u8 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub temporal_endpoint: String,
    pub temporal_namespace: String,
    pub lifecycle_task_queue: String,
    pub settlement_task_queue: String,
    pub auction_endpoint: String,
    pub payments_endpoint: String,
    pub notify_endpoint: String,
    pub auction_principal_id: String,
    pub workload_spiffe_id: String,
    pub temporal_api_key: Option<String>,
    pub http_addr: SocketAddr,
    pub graceful_shutdown: Duration,
    pub redpanda_brokers: String,
    pub redpanda_group_id: String,
    pub redpanda_client_id: String,
    pub redpanda_security_protocol: String,
    pub redpanda_sasl_username: Option<String>,
    pub redpanda_sasl_password: Option<String>,
    pub quarantine_timeout: Duration,
    pub broker_retry_attempts: u32,
    pub broker_retry_initial: Duration,
    pub broker_retry_max: Duration,
}

impl RuntimeConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_values(std::env::vars().collect())
    }

    pub fn from_values(values: HashMap<String, String>) -> Result<Self, ConfigError> {
        let required = |key: &'static str| {
            values
                .get(key)
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.trim().to_owned())
                .ok_or(ConfigError::MissingValue(key))
        };
        let port = values
            .get("PORT")
            .map_or(Ok(8080), |value| value.parse::<u16>())
            .map_err(|_| ConfigError::InvalidValue("PORT"))?;
        let shutdown_seconds = values
            .get("GRACEFUL_SHUTDOWN_SECONDS")
            .map_or(Ok(20), |value| value.parse::<u64>())
            .map_err(|_| ConfigError::InvalidValue("GRACEFUL_SHUTDOWN_SECONDS"))?;
        let quarantine_timeout_ms = parse_u64(&values, "QUARANTINE_TIMEOUT_MS", 3_000)?;
        let broker_retry_attempts = parse_u32(&values, "BROKER_RETRY_ATTEMPTS", 4)?;
        let broker_retry_initial_ms = parse_u64(&values, "BROKER_RETRY_INITIAL_MS", 100)?;
        let broker_retry_max_ms = parse_u64(&values, "BROKER_RETRY_MAX_MS", 2_000)?;
        let config = Self {
            temporal_endpoint: required("TEMPORAL_ENDPOINT")?,
            temporal_namespace: required("TEMPORAL_NAMESPACE")?,
            lifecycle_task_queue: required("TEMPORAL_LIFECYCLE_TASK_QUEUE")?,
            settlement_task_queue: required("TEMPORAL_SETTLEMENT_TASK_QUEUE")?,
            auction_endpoint: required("AUCTION_GRPC_ENDPOINT")?,
            payments_endpoint: required("PAYMENTS_GRPC_ENDPOINT")?,
            notify_endpoint: required("NOTIFY_GRPC_ENDPOINT")?,
            auction_principal_id: required("AUCTION_PRINCIPAL_ID")?,
            workload_spiffe_id: required("PROCESSES_SPIFFE_ID")?,
            temporal_api_key: values
                .get("TEMPORAL_API_KEY")
                .filter(|value| !value.trim().is_empty())
                .cloned(),
            http_addr: SocketAddr::from(([0, 0, 0, 0], port)),
            graceful_shutdown: Duration::from_secs(shutdown_seconds),
            redpanda_brokers: required("REDPANDA_BROKERS")?,
            redpanda_group_id: values.get("REDPANDA_GROUP_ID").map_or_else(
                || "gavel-processes-v1".to_owned(),
                |value| value.trim().to_owned(),
            ),
            redpanda_client_id: values.get("REDPANDA_CLIENT_ID").map_or_else(
                || "gavel-processes".to_owned(),
                |value| value.trim().to_owned(),
            ),
            redpanda_security_protocol: values.get("REDPANDA_SECURITY_PROTOCOL").map_or_else(
                || "SASL_PLAINTEXT".to_owned(),
                |value| value.trim().to_owned(),
            ),
            redpanda_sasl_username: optional(&values, "REDPANDA_SASL_USERNAME"),
            redpanda_sasl_password: optional(&values, "REDPANDA_SASL_PASSWORD"),
            quarantine_timeout: Duration::from_millis(quarantine_timeout_ms),
            broker_retry_attempts,
            broker_retry_initial: Duration::from_millis(broker_retry_initial_ms),
            broker_retry_max: Duration::from_millis(broker_retry_max_ms),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        for (key, value) in [
            ("TEMPORAL_ENDPOINT", &self.temporal_endpoint),
            ("AUCTION_GRPC_ENDPOINT", &self.auction_endpoint),
            ("PAYMENTS_GRPC_ENDPOINT", &self.payments_endpoint),
            ("NOTIFY_GRPC_ENDPOINT", &self.notify_endpoint),
        ] {
            let url = url::Url::parse(value).map_err(|_| ConfigError::InvalidValue(key))?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err(ConfigError::InvalidValue(key));
            }
        }
        if self.lifecycle_task_queue == self.settlement_task_queue {
            return Err(ConfigError::QueuesMustDiffer);
        }
        if !self.workload_spiffe_id.starts_with("spiffe://") || self.workload_spiffe_id.len() > 256
        {
            return Err(ConfigError::InvalidValue("PROCESSES_SPIFFE_ID"));
        }
        if self.graceful_shutdown.is_zero() {
            return Err(ConfigError::InvalidValue("GRACEFUL_SHUTDOWN_SECONDS"));
        }
        if self.redpanda_brokers.is_empty()
            || self.redpanda_group_id.is_empty()
            || self.redpanda_client_id.is_empty()
            || self.quarantine_timeout.is_zero()
            || self.broker_retry_attempts == 0
            || self.broker_retry_initial.is_zero()
            || self.broker_retry_max.is_zero()
            || self.broker_retry_initial > self.broker_retry_max
            || self.redpanda_sasl_username.is_some() != self.redpanda_sasl_password.is_some()
        {
            return Err(ConfigError::InvalidValue("REDPANDA_CONFIGURATION"));
        }
        Ok(())
    }
}

fn optional(values: &HashMap<String, String>, key: &str) -> Option<String> {
    values
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_u64(
    values: &HashMap<String, String>,
    key: &'static str,
    default: u64,
) -> Result<u64, ConfigError> {
    values
        .get(key)
        .map_or(Ok(default), |value| value.parse())
        .map_err(|_| ConfigError::InvalidValue(key))
}

fn parse_u32(
    values: &HashMap<String, String>,
    key: &'static str,
    default: u32,
) -> Result<u32, ConfigError> {
    values
        .get(key)
        .map_or(Ok(default), |value| value.parse())
        .map_err(|_| ConfigError::InvalidValue(key))
}

/// Process-level health is intentionally independent from any workflow or entity identifier.
/// Readiness covers Temporal, broker/topics, consumer loop, owner RPC transports and workers.
#[derive(Debug)]
pub struct RuntimeHealth {
    live: AtomicBool,
    temporal_connected: AtomicBool,
    broker_connected: AtomicBool,
    auction_rpc_ready: AtomicBool,
    payments_rpc_ready: AtomicBool,
    notify_rpc_ready: AtomicBool,
    consumer_running: AtomicBool,
    stopping: AtomicBool,
    active_workers: AtomicU8,
    published_ready: AtomicBool,
}

impl Default for RuntimeHealth {
    fn default() -> Self {
        Self {
            live: AtomicBool::new(true),
            temporal_connected: AtomicBool::new(false),
            broker_connected: AtomicBool::new(false),
            auction_rpc_ready: AtomicBool::new(false),
            payments_rpc_ready: AtomicBool::new(false),
            notify_rpc_ready: AtomicBool::new(false),
            consumer_running: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            active_workers: AtomicU8::new(0),
            published_ready: AtomicBool::new(false),
        }
    }
}

impl RuntimeHealth {
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.is_live()
            && !self.stopping.load(Ordering::Acquire)
            && self.temporal_connected.load(Ordering::Acquire)
            && self.broker_connected.load(Ordering::Acquire)
            && self.auction_rpc_ready.load(Ordering::Acquire)
            && self.payments_rpc_ready.load(Ordering::Acquire)
            && self.notify_rpc_ready.load(Ordering::Acquire)
            && self.consumer_running.load(Ordering::Acquire)
            && self.active_workers.load(Ordering::Acquire) == EXPECTED_WORKERS
    }

    pub fn set_temporal_connected(&self, connected: bool) {
        self.temporal_connected.store(connected, Ordering::Release);
        self.publish_dependency("temporal", connected);
        self.publish_readiness();
    }

    pub fn set_broker_connected(&self, connected: bool) {
        self.broker_connected.store(connected, Ordering::Release);
        self.publish_dependency("broker", connected);
        self.publish_readiness();
    }

    pub fn set_auction_rpc_ready(&self, ready: bool) {
        self.auction_rpc_ready.store(ready, Ordering::Release);
        self.publish_dependency("auction_rpc", ready);
        self.publish_readiness();
    }

    pub fn set_payments_rpc_ready(&self, ready: bool) {
        self.payments_rpc_ready.store(ready, Ordering::Release);
        self.publish_dependency("payments_rpc", ready);
        self.publish_readiness();
    }

    pub fn set_notify_rpc_ready(&self, ready: bool) {
        self.notify_rpc_ready.store(ready, Ordering::Release);
        self.publish_dependency("notify_rpc", ready);
        self.publish_readiness();
    }

    pub fn consumer_started(&self) -> ConsumerActivityGuard<'_> {
        self.consumer_running.store(true, Ordering::Release);
        self.publish_dependency("broker_consumer", true);
        self.publish_readiness();
        ConsumerActivityGuard { health: self }
    }

    #[must_use]
    pub fn readiness_snapshot(&self) -> RuntimeReadiness {
        RuntimeReadiness {
            ready: self.is_ready(),
            temporal: self.temporal_connected.load(Ordering::Acquire),
            broker: self.broker_connected.load(Ordering::Acquire),
            broker_consumer: self.consumer_running.load(Ordering::Acquire),
            auction_rpc: self.auction_rpc_ready.load(Ordering::Acquire),
            payments_rpc: self.payments_rpc_ready.load(Ordering::Acquire),
            notify_rpc: self.notify_rpc_ready.load(Ordering::Acquire),
            workers: self.active_workers.load(Ordering::Acquire),
        }
    }

    /// Marks a worker run loop active and returns a guard that removes it on every exit path.
    pub fn worker_started(&self) -> WorkerActivityGuard<'_> {
        let previous = self.active_workers.fetch_add(1, Ordering::AcqRel);
        debug_assert!(
            previous < EXPECTED_WORKERS,
            "too many process workers started"
        );
        self.publish_readiness();
        WorkerActivityGuard { health: self }
    }

    pub fn begin_shutdown(&self) {
        self.stopping.store(true, Ordering::Release);
        self.publish_readiness();
    }

    pub fn mark_stopped(&self) {
        self.live.store(false, Ordering::Release);
        self.publish_readiness();
    }

    fn worker_stopped(&self) {
        let previous = self.active_workers.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "process worker stopped without being started");
        self.publish_readiness();
    }

    fn publish_readiness(&self) {
        let ready = self.is_ready();
        if self.published_ready.swap(ready, Ordering::AcqRel) != ready {
            metrics::gauge!("gavel_process_worker_ready").set(u8::from(ready) as f64);
        }
    }

    fn publish_dependency(&self, dependency: &'static str, ready: bool) {
        metrics::gauge!(
            "gavel_process_dependency_ready",
            "dependency" => dependency
        )
        .set(u8::from(ready) as f64);
    }

    fn consumer_stopped(&self) {
        self.consumer_running.store(false, Ordering::Release);
        self.publish_dependency("broker_consumer", false);
        self.publish_readiness();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeReadiness {
    pub ready: bool,
    pub temporal: bool,
    pub broker: bool,
    pub broker_consumer: bool,
    pub auction_rpc: bool,
    pub payments_rpc: bool,
    pub notify_rpc: bool,
    pub workers: u8,
}

#[must_use]
pub struct WorkerActivityGuard<'a> {
    health: &'a RuntimeHealth,
}

impl Drop for WorkerActivityGuard<'_> {
    fn drop(&mut self) {
        self.health.worker_stopped();
    }
}

#[must_use]
pub struct ConsumerActivityGuard<'a> {
    health: &'a RuntimeHealth,
}

impl Drop for ConsumerActivityGuard<'_> {
    fn drop(&mut self) {
        self.health.consumer_stopped();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("обязательная переменная окружения отсутствует: {0}")]
    MissingValue(&'static str),
    #[error("переменная окружения имеет недопустимое значение: {0}")]
    InvalidValue(&'static str),
    #[error("очереди lifecycle и settlement должны различаться")]
    QueuesMustDiffer,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> HashMap<String, String> {
        HashMap::from([
            (
                "TEMPORAL_ENDPOINT".to_owned(),
                "http://temporal:7233".to_owned(),
            ),
            ("TEMPORAL_NAMESPACE".to_owned(), "processes".to_owned()),
            (
                "TEMPORAL_LIFECYCLE_TASK_QUEUE".to_owned(),
                "auction-lifecycle-v1".to_owned(),
            ),
            (
                "TEMPORAL_SETTLEMENT_TASK_QUEUE".to_owned(),
                "settle-auction-v1".to_owned(),
            ),
            (
                "AUCTION_GRPC_ENDPOINT".to_owned(),
                "http://auction:50051".to_owned(),
            ),
            (
                "PAYMENTS_GRPC_ENDPOINT".to_owned(),
                "http://payments:50051".to_owned(),
            ),
            (
                "NOTIFY_GRPC_ENDPOINT".to_owned(),
                "http://notify:50051".to_owned(),
            ),
            ("AUCTION_PRINCIPAL_ID".to_owned(), "processes".to_owned()),
            ("REDPANDA_BROKERS".to_owned(), "redpanda:9092".to_owned()),
        ])
    }

    #[test]
    fn validates_complete_configuration_and_rejects_unsafe_startup() {
        assert!(RuntimeConfig::from_values(values()).is_ok());
        let mut missing = values();
        missing.remove("TEMPORAL_NAMESPACE");
        assert_eq!(
            RuntimeConfig::from_values(missing),
            Err(ConfigError::MissingValue("TEMPORAL_NAMESPACE"))
        );
        let mut same_queues = values();
        same_queues.insert(
            "TEMPORAL_SETTLEMENT_TASK_QUEUE".to_owned(),
            "auction-lifecycle-v1".to_owned(),
        );
        assert_eq!(
            RuntimeConfig::from_values(same_queues),
            Err(ConfigError::QueuesMustDiffer)
        );

        let mut unsafe_scheme = values();
        unsafe_scheme.insert(
            "TEMPORAL_ENDPOINT".to_owned(),
            "file:///tmp/socket".to_owned(),
        );
        assert_eq!(
            RuntimeConfig::from_values(unsafe_scheme),
            Err(ConfigError::InvalidValue("TEMPORAL_ENDPOINT"))
        );

        let mut zero_drain = values();
        zero_drain.insert("GRACEFUL_SHUTDOWN_SECONDS".to_owned(), "0".to_owned());
        assert_eq!(
            RuntimeConfig::from_values(zero_drain),
            Err(ConfigError::InvalidValue("GRACEFUL_SHUTDOWN_SECONDS"))
        );
    }

    #[test]
    fn readiness_requires_temporal_and_both_workers_and_drops_on_disconnect_or_shutdown() {
        let health = RuntimeHealth::default();
        assert!(health.is_live());
        assert!(!health.is_ready());

        health.set_temporal_connected(true);
        health.set_broker_connected(true);
        health.set_auction_rpc_ready(true);
        health.set_payments_rpc_ready(true);
        health.set_notify_rpc_ready(true);
        let consumer = health.consumer_started();
        let lifecycle = health.worker_started();
        assert!(!health.is_ready());
        let settlement = health.worker_started();
        assert!(health.is_ready());

        health.set_broker_connected(false);
        assert!(!health.is_ready());
        health.set_broker_connected(true);
        health.set_auction_rpc_ready(false);
        assert!(!health.is_ready());
        health.set_auction_rpc_ready(true);
        health.set_payments_rpc_ready(false);
        assert!(!health.is_ready());
        health.set_payments_rpc_ready(true);
        health.set_notify_rpc_ready(false);
        assert!(!health.is_ready());
        health.set_notify_rpc_ready(true);
        assert!(health.is_ready());
        assert_eq!(health.readiness_snapshot().workers, 2);

        health.set_temporal_connected(false);
        assert!(!health.is_ready());
        health.set_temporal_connected(true);
        assert!(health.is_ready());

        drop(settlement);
        assert!(!health.is_ready());
        drop(lifecycle);
        drop(consumer);
        health.begin_shutdown();
        health.mark_stopped();
        assert!(!health.is_live());
        assert!(!health.is_ready());
    }
}
