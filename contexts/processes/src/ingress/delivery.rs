//! At-least-once broker delivery policy and privacy-safe quarantine/replay boundary.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    BrokerRecord, EventDecodeError, EventSource, IngressDisposition, SignalIngress,
    decode_broker_record,
};
use crate::workflows::ProcessError;

#[derive(Clone, PartialEq, Eq)]
pub struct ConsumedRecord {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub key: Option<Vec<u8>>,
    pub payload: Vec<u8>,
}

impl std::fmt::Debug for ConsumedRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConsumedRecord")
            .field("topic", &self.topic)
            .field("partition", &self.partition)
            .field("offset", &self.offset)
            .field("key", &"[REDACTED]")
            .field("payload", &"[REDACTED]")
            .finish()
    }
}

impl ConsumedRecord {
    pub fn source(&self) -> Result<EventSource, DeliveryError> {
        match self.topic.as_str() {
            "gavel.auction.public.v1" => Ok(EventSource::AuctionPublic),
            "gavel.auction.processes.v1" => Ok(EventSource::AuctionRestricted),
            "gavel.payments.attempts.v1" => Ok(EventSource::PaymentsAttempt),
            _ => Err(DeliveryError::PermanentRouting),
        }
    }

    fn broker_record(&self) -> Result<BrokerRecord, DeliveryError> {
        Ok(BrokerRecord {
            source: self.source()?,
            payload: self.payload.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl RetryPolicy {
    pub fn validate(self) -> Result<Self, DeliveryError> {
        if self.max_attempts == 0 || self.initial_backoff.is_zero() || self.max_backoff.is_zero() {
            return Err(DeliveryError::InvalidConfiguration);
        }
        Ok(self)
    }

    fn delay(self, failed_attempt: u32) -> Duration {
        let multiplier = 1_u32
            .checked_shl(failed_attempt.saturating_sub(1).min(16))
            .unwrap_or(u32::MAX);
        self.initial_backoff
            .saturating_mul(multiplier)
            .min(self.max_backoff)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeliveryError {
    #[error("broker operation failed")]
    Broker,
    #[error("quarantine publication failed")]
    QuarantineUnavailable,
    #[error("retry budget was exhausted")]
    RetryExhausted,
    #[error("record has no registered route")]
    PermanentRouting,
    #[error("delivery configuration is invalid")]
    InvalidConfiguration,
    #[error("quarantine record was not found")]
    ReplayNotFound,
    #[error("quarantine replay is still invalid")]
    ReplayRejected,
}

#[async_trait]
pub trait BrokerDelivery: Send + Sync {
    async fn receive(&self) -> Result<ConsumedRecord, DeliveryError>;
    async fn commit(&self, record: &ConsumedRecord) -> Result<(), DeliveryError>;
    async fn quarantine(&self, record: &QuarantineRecord) -> Result<(), DeliveryError>;
}

#[async_trait]
pub trait Backoff: Send + Sync {
    async fn wait(&self, duration: Duration);
}

#[derive(Clone, Copy, Default)]
pub struct TokioBackoff;

#[async_trait]
impl Backoff for TokioBackoff {
    async fn wait(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QuarantineCategory {
    InvalidEnvelope,
    UnsupportedSchemaVersion,
    PermanentRoutingFailure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarantineRecord {
    pub record_id: String,
    pub schema_version: u32,
    pub occurred_at: String,
    pub source: QuarantineSource,
    pub failure: QuarantineFailure,
    pub replay: QuarantineReplay,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarantineSource {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub event_id: String,
    pub partition_key_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarantineFailure {
    pub category: QuarantineCategory,
    pub safe_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReplayStatus {
    Pending,
    Replayed,
    Discarded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarantineReplay {
    pub status: ReplayStatus,
    pub attempts: u32,
}

impl QuarantineRecord {
    pub fn sanitized(record: &ConsumedRecord, category: QuarantineCategory) -> Self {
        let location = format!("{}:{}:{}", record.topic, record.partition, record.offset);
        let key_material = record.key.as_deref().unwrap_or(location.as_bytes());
        let payload_fingerprint = hex_digest(&record.payload);
        Self {
            record_id: format!("quarantine-{}", &hex_digest(location.as_bytes())[..24]),
            schema_version: 1,
            occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            source: QuarantineSource {
                topic: record.topic.clone(),
                partition: record.partition,
                offset: record.offset,
                // Invalid bytes are never parsed into diagnostics. A stable opaque token
                // supports correlation without exposing candidate or provider data.
                event_id: format!("opaque-{}", &payload_fingerprint[..24]),
                partition_key_hash: hex_digest(key_material),
            },
            failure: QuarantineFailure {
                category,
                safe_code: match category {
                    QuarantineCategory::InvalidEnvelope => "INVALID_ENVELOPE",
                    QuarantineCategory::UnsupportedSchemaVersion => "UNSUPPORTED_SCHEMA_VERSION",
                    QuarantineCategory::PermanentRoutingFailure => "PERMANENT_ROUTING_FAILURE",
                }
                .to_owned(),
            },
            replay: QuarantineReplay {
                status: ReplayStatus::Pending,
                attempts: 0,
            },
        }
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub struct ConsumerLoop<B, I, S = TokioBackoff> {
    broker: Arc<B>,
    ingress: Arc<I>,
    retry: RetryPolicy,
    backoff: S,
}

impl<B, I> ConsumerLoop<B, I, TokioBackoff>
where
    B: BrokerDelivery,
    I: SignalIngress,
{
    pub fn new(broker: Arc<B>, ingress: Arc<I>, retry: RetryPolicy) -> Result<Self, DeliveryError> {
        Ok(Self {
            broker,
            ingress,
            retry: retry.validate()?,
            backoff: TokioBackoff,
        })
    }
}

impl<B, I, S> ConsumerLoop<B, I, S>
where
    B: BrokerDelivery,
    I: SignalIngress,
    S: Backoff,
{
    pub fn with_backoff(
        broker: Arc<B>,
        ingress: Arc<I>,
        retry: RetryPolicy,
        backoff: S,
    ) -> Result<Self, DeliveryError> {
        Ok(Self {
            broker,
            ingress,
            retry: retry.validate()?,
            backoff,
        })
    }

    /// Handles one record. Commit is always the final operation, after either a
    /// durable workflow outcome or a durably published sanitized quarantine record.
    pub async fn run_once(&self) -> Result<IngressDisposition, DeliveryError> {
        let record = self.broker.receive().await?;
        record_delivery(&record, "received");
        let disposition = self.process(&record).await.inspect_err(|_| {
            record_delivery(&record, "failed");
        })?;
        self.broker.commit(&record).await.inspect_err(|_| {
            record_delivery(&record, "commit_failed");
        })?;
        record_delivery(&record, "committed");
        Ok(disposition)
    }

    /// Runs until graceful shutdown. A delivery error stops the loop so the
    /// supervisor restarts the same stable consumer group from its last commit.
    pub async fn run(
        &self,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> Result<(), DeliveryError> {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                result = self.run_once() => {
                    result?;
                }
            }
        }
    }

    async fn process(&self, record: &ConsumedRecord) -> Result<IngressDisposition, DeliveryError> {
        let broker_record = match record.broker_record() {
            Ok(value) => value,
            Err(DeliveryError::PermanentRouting) => {
                return self
                    .publish_quarantine(record, QuarantineCategory::PermanentRoutingFailure)
                    .await;
            }
            Err(error) => return Err(error),
        };
        if let Err(error) = decode_broker_record(&broker_record) {
            let category = match error {
                EventDecodeError::UnsupportedSchemaVersion(_) => {
                    QuarantineCategory::UnsupportedSchemaVersion
                }
                _ => QuarantineCategory::InvalidEnvelope,
            };
            return self.publish_quarantine(record, category).await;
        }
        for attempt in 1..=self.retry.max_attempts {
            match self.ingress.ingest(broker_record.clone()).await {
                Ok(IngressDisposition::Quarantined) => {
                    return self
                        .publish_quarantine(record, QuarantineCategory::PermanentRoutingFailure)
                        .await;
                }
                Ok(disposition) => return Ok(disposition),
                Err(ProcessError::DependencyUnavailable) if attempt < self.retry.max_attempts => {
                    record_delivery(record, "retry");
                    self.backoff.wait(self.retry.delay(attempt)).await;
                }
                Err(ProcessError::DependencyUnavailable) => {
                    return Err(DeliveryError::RetryExhausted);
                }
                Err(ProcessError::InvalidInput) => {
                    return self
                        .publish_quarantine(record, QuarantineCategory::InvalidEnvelope)
                        .await;
                }
                Err(
                    ProcessError::InvariantViolation
                    | ProcessError::ChangedRedelivery
                    | ProcessError::OwnerRejected { .. }
                    | ProcessError::StaleEvent,
                ) => {
                    return self
                        .publish_quarantine(record, QuarantineCategory::PermanentRoutingFailure)
                        .await;
                }
            }
        }
        Err(DeliveryError::RetryExhausted)
    }

    async fn publish_quarantine(
        &self,
        record: &ConsumedRecord,
        category: QuarantineCategory,
    ) -> Result<IngressDisposition, DeliveryError> {
        self.broker
            .quarantine(&QuarantineRecord::sanitized(record, category))
            .await?;
        record_delivery(record, "quarantined");
        Ok(IngressDisposition::Quarantined)
    }
}

fn record_delivery(record: &ConsumedRecord, outcome: &'static str) {
    let (owner, route) = match record.topic.as_str() {
        "gavel.auction.public.v1" => ("auction", "public"),
        "gavel.auction.processes.v1" => ("auction", "restricted"),
        "gavel.payments.attempts.v1" => ("payments", "restricted"),
        _ => ("unknown", "quarantine"),
    };
    let outcome = match outcome {
        "received" | "retry" | "quarantined" | "committed" | "commit_failed" | "failed" => outcome,
        _ => "unknown",
    };
    metrics::counter!(
        "gavel_process_ingress_delivery_total",
        "owner" => owner,
        "route" => route,
        "outcome" => outcome
    )
    .increment(1);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayQuarantineCommand {
    pub record_id: String,
}

#[async_trait]
pub trait ReplayArchive: Send + Sync {
    async fn load_original(&self, record_id: &str) -> Result<ConsumedRecord, DeliveryError>;
    async fn mark_replayed(&self, record_id: &str) -> Result<(), DeliveryError>;
}

pub struct QuarantineReplayService<A, I> {
    archive: Arc<A>,
    ingress: Arc<I>,
}

impl<A, I> QuarantineReplayService<A, I>
where
    A: ReplayArchive,
    I: SignalIngress,
{
    pub fn new(archive: Arc<A>, ingress: Arc<I>) -> Self {
        Self { archive, ingress }
    }

    pub async fn replay(
        &self,
        command: ReplayQuarantineCommand,
    ) -> Result<IngressDisposition, DeliveryError> {
        if command.record_id.trim().is_empty() {
            return Err(DeliveryError::ReplayNotFound);
        }
        let original = self.archive.load_original(&command.record_id).await?;
        let record = original.broker_record()?;
        if decode_broker_record(&record).is_err() {
            return Err(DeliveryError::ReplayRejected);
        }
        let disposition = self
            .ingress
            .ingest(record)
            .await
            .map_err(|error| match error {
                ProcessError::DependencyUnavailable => DeliveryError::RetryExhausted,
                _ => DeliveryError::ReplayRejected,
            })?;
        if disposition == IngressDisposition::Quarantined {
            return Err(DeliveryError::ReplayRejected);
        }
        self.archive.mark_replayed(&command.record_id).await?;
        Ok(disposition)
    }
}
