use std::sync::Arc;

use async_trait::async_trait;

use super::{
    DecodedOwnerEvent, EventDecodeError, EventGate, EventSource, GateDecision,
    SignalWithStartClient, TemporalGateway, decode_auction_process, decode_auction_public,
    decode_payment_event,
};
use crate::{
    telemetry::{MetricsTelemetry, ProcessTelemetry},
    workflows::ProcessError,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokerRecord {
    pub source: EventSource,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngressDisposition {
    Signalled {
        workflow_id: String,
    },
    SignalledAfterGap {
        workflow_id: String,
        expected_version: u64,
        received_version: u64,
    },
    Duplicate,
    Stale,
    Ignored,
    Quarantined,
}

#[async_trait]
pub trait SignalIngress: Send + Sync {
    async fn ingest(&self, record: BrokerRecord) -> Result<IngressDisposition, ProcessError>;
}

pub fn decode_broker_record(record: &BrokerRecord) -> Result<DecodedOwnerEvent, EventDecodeError> {
    match record.source {
        EventSource::AuctionPublic => decode_auction_public(&record.payload),
        EventSource::AuctionRestricted => decode_auction_process(&record.payload),
        EventSource::PaymentsAttempt => decode_payment_event(&record.payload),
    }
}

pub struct VersionedSignalIngress<C> {
    gate: tokio::sync::Mutex<EventGate>,
    gateway: TemporalGateway<C>,
    telemetry: Arc<dyn ProcessTelemetry>,
}

impl<C> VersionedSignalIngress<C>
where
    C: SignalWithStartClient,
{
    pub fn new(gateway: TemporalGateway<C>) -> Self {
        Self {
            gate: tokio::sync::Mutex::new(EventGate::default()),
            gateway,
            telemetry: Arc::new(MetricsTelemetry),
        }
    }

    pub fn with_telemetry(mut self, telemetry: Arc<dyn ProcessTelemetry>) -> Self {
        self.telemetry = telemetry;
        self
    }
}

#[async_trait]
impl<C> SignalIngress for VersionedSignalIngress<C>
where
    C: SignalWithStartClient,
{
    async fn ingest(&self, record: BrokerRecord) -> Result<IngressDisposition, ProcessError> {
        let decoded = decode_broker_record(&record).map_err(|_| {
            self.telemetry.signal("decode", "invalid");
            ProcessError::InvalidInput
        })?;
        let event_type = event_type(&decoded.event);

        // Serialize inspection + durable signal + gate commit. The broker can still
        // process separate partitions concurrently with separate ingress instances;
        // a single instance never advances its mirror before Temporal acceptance.
        let mut gate = self.gate.lock().await;
        let decision = gate.inspect(record.source, &decoded, &record.payload)?;
        match decision {
            GateDecision::Duplicate => {
                self.telemetry.signal(event_type, "duplicate");
                Ok(IngressDisposition::Duplicate)
            }
            GateDecision::Stale { .. } => {
                self.telemetry.signal(event_type, "stale");
                Ok(IngressDisposition::Stale)
            }
            GateDecision::Quarantined => {
                // Do not advance the in-memory mirror here. The delivery layer must
                // durably publish the sanitized quarantine record before committing
                // the broker offset; if publication fails, a retry must remain
                // quarantined instead of being misclassified as a duplicate.
                self.telemetry.signal(event_type, "quarantined");
                Ok(IngressDisposition::Quarantined)
            }
            GateDecision::Accepted => {
                if matches!(&decoded.event, super::OwnerEvent::AuctionObserved { .. }) {
                    gate.admit(record.source, &decoded, &record.payload)?;
                    self.telemetry.signal(event_type, "ignored");
                    return Ok(IngressDisposition::Ignored);
                }
                let receipt = self.gateway.signal(decoded.clone()).await?;
                gate.admit(record.source, &decoded, &record.payload)?;
                self.telemetry.signal(event_type, "applied");
                Ok(IngressDisposition::Signalled {
                    workflow_id: receipt.workflow_id,
                })
            }
            GateDecision::Gap {
                expected_version,
                received_version,
            } => {
                let receipt = self.gateway.signal(decoded.clone()).await?;
                gate.admit(record.source, &decoded, &record.payload)?;
                self.telemetry.signal(event_type, "gap");
                Ok(IngressDisposition::SignalledAfterGap {
                    workflow_id: receipt.workflow_id,
                    expected_version,
                    received_version,
                })
            }
        }
    }
}

const fn event_type(event: &super::OwnerEvent) -> &'static str {
    match event {
        super::OwnerEvent::AuctionObserved { .. } => "auction_observed",
        super::OwnerEvent::AuctionOpened { .. } => "auction_opened",
        super::OwnerEvent::AuctionDeadlineChanged { .. } => "auction_deadline_changed",
        super::OwnerEvent::AuctionClosed { .. } => "auction_closed",
        super::OwnerEvent::AuctionCancelled { .. } => "auction_cancelled",
        super::OwnerEvent::AuctionUnsold { .. } => "auction_unsold",
        super::OwnerEvent::WinnerConfirmed { .. } => "winner_confirmed",
        super::OwnerEvent::SettlementCandidateSelected { .. } => "candidate_selected",
        super::OwnerEvent::PaymentAttemptStarted { .. } => "payment_attempt_started",
        super::OwnerEvent::PaymentRecoveryRequired { .. } => "payment_recovery_required",
        super::OwnerEvent::PaymentSucceeded { .. } => "payment_succeeded",
        super::OwnerEvent::PaymentExpired { .. } => "payment_expired",
    }
}

#[derive(Clone)]
pub struct TestBrokerAdapter<I> {
    ingress: Arc<I>,
}

impl<I> TestBrokerAdapter<I>
where
    I: SignalIngress,
{
    pub fn new(ingress: Arc<I>) -> Self {
        Self { ingress }
    }

    pub async fn deliver(&self, record: BrokerRecord) -> Result<IngressDisposition, ProcessError> {
        self.ingress.ingest(record).await
    }
}
