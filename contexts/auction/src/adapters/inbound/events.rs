use std::{fmt::Write, sync::Arc};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    application::{
        ApplicationError, AuctionUnitOfWork, Inbox, InboxError, InboxOutcome, LotListed,
        OpenAuction, PaymentReadinessStore, PaymentReadinessUpdate, PaymentReadinessUpdateOutcome,
        PortError,
    },
    domain::{ClosingPolicy, DurationMillis, Timestamp},
    telemetry::{ConsumerMetricOutcome, record_consumer},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeOutcome {
    Applied,
    Duplicate,
    Stale,
    Quarantined,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConsumerError {
    #[error("event payload is malformed")]
    Malformed,
    #[error("event schema or type is unsupported")]
    Unsupported,
    #[error("consumer storage is unavailable")]
    Unavailable,
    #[error("event handler rejected the message")]
    Rejected,
    #[error("consumer failed")]
    Internal,
}

pub struct CatalogEventConsumer<U, I> {
    inbox: Arc<I>,
    open_auction: OpenAuction<U>,
}

impl<U, I> CatalogEventConsumer<U, I>
where
    U: AuctionUnitOfWork,
    I: Inbox,
{
    pub fn new(
        unit_of_work: Arc<U>,
        inbox: Arc<I>,
        retry_attempts: usize,
        fingerprint_key: [u8; 32],
    ) -> Self {
        Self {
            inbox,
            open_auction: OpenAuction::new(unit_of_work, retry_attempts, fingerprint_key),
        }
    }

    pub async fn handle(&self, bytes: &[u8]) -> Result<ConsumeOutcome, ConsumerError> {
        let envelope: EventEnvelope<CatalogPayload> =
            serde_json::from_slice(bytes).map_err(|_| ConsumerError::Malformed)?;
        if envelope.schema_version != 1 || envelope.event_type != "LotListed" {
            return Err(ConsumerError::Unsupported);
        }
        validate_envelope(&envelope)?;
        if envelope.payload.auction_id.trim().is_empty()
            || envelope.payload.lot_id.trim().is_empty()
            || envelope.payload.seller_id.trim().is_empty()
            || envelope.payload.currency_code.trim().is_empty()
        {
            return Err(ConsumerError::Malformed);
        }
        let policy = ClosingPolicy::new(
            envelope
                .payload
                .closing_policy
                .scheduled_end_millis
                .map(Timestamp::from_millis),
            envelope
                .payload
                .closing_policy
                .inactivity_millis
                .and_then(DurationMillis::new),
            envelope
                .payload
                .closing_policy
                .hard_deadline_millis
                .map(Timestamp::from_millis),
        )
        .map_err(|_| ConsumerError::Malformed)?;
        let inbox = self
            .inbox
            .accept(
                "auction.catalog.v1",
                &envelope.event_id,
                &fingerprint(bytes),
            )
            .await
            .map_err(map_inbox_error)?;
        match inbox {
            InboxOutcome::Duplicate => return Ok(observe(ConsumeOutcome::Duplicate)),
            InboxOutcome::PayloadConflict => return Ok(observe(ConsumeOutcome::Quarantined)),
            InboxOutcome::Accepted => {}
        }
        self.open_auction
            .handle(LotListed {
                event_id: envelope.event_id,
                correlation_id: envelope.correlation_id,
                auction_id: envelope.payload.auction_id,
                lot_id: envelope.payload.lot_id,
                seller_id: envelope.payload.seller_id,
                start_price_minor: envelope.payload.start_price_minor,
                currency_code: envelope.payload.currency_code,
                bid_step_minor: envelope.payload.bid_step_minor,
                closing_policy: policy,
                listed_at: Timestamp::from_millis(envelope.occurred_at_millis),
            })
            .await
            .map_err(map_application_error)?;
        Ok(observe(ConsumeOutcome::Applied))
    }
}

pub struct PaymentsEventConsumer<I, S> {
    inbox: Arc<I>,
    readiness: Arc<S>,
}

impl<I, S> PaymentsEventConsumer<I, S>
where
    I: Inbox,
    S: PaymentReadinessStore,
{
    pub fn new(inbox: Arc<I>, readiness: Arc<S>) -> Self {
        Self { inbox, readiness }
    }

    pub async fn handle(&self, bytes: &[u8]) -> Result<ConsumeOutcome, ConsumerError> {
        let envelope: EventEnvelope<PaymentPayload> =
            serde_json::from_slice(bytes).map_err(|_| ConsumerError::Malformed)?;
        if envelope.schema_version != 1
            || !matches!(
                envelope.event_type.as_str(),
                "PaymentInstrumentReady" | "PaymentInstrumentRevoked"
            )
        {
            return Err(ConsumerError::Unsupported);
        }
        validate_envelope(&envelope)?;
        if envelope.payload.user_id.trim().is_empty()
            || envelope
                .payload
                .instrument_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            || envelope
                .aggregate_id
                .as_deref()
                .is_some_and(|aggregate_id| {
                    envelope.payload.instrument_id.as_deref() != Some(aggregate_id)
                })
        {
            return Err(ConsumerError::Malformed);
        }
        match self
            .inbox
            .accept(
                "auction.payments.v1",
                &envelope.event_id,
                &fingerprint(bytes),
            )
            .await
            .map_err(map_inbox_error)?
        {
            InboxOutcome::Duplicate => return Ok(observe(ConsumeOutcome::Duplicate)),
            InboxOutcome::PayloadConflict => return Ok(observe(ConsumeOutcome::Quarantined)),
            InboxOutcome::Accepted => {}
        }
        let ready = envelope.event_type == "PaymentInstrumentReady";
        let outcome = self
            .readiness
            .apply_update(PaymentReadinessUpdate {
                user_id: envelope.payload.user_id,
                ready,
                source_version: envelope.aggregate_version,
                source_event_id: envelope.event_id,
                occurred_at: Timestamp::from_millis(envelope.occurred_at_millis),
            })
            .await
            .map_err(map_port_error)?;
        Ok(observe(match outcome {
            PaymentReadinessUpdateOutcome::Applied => ConsumeOutcome::Applied,
            PaymentReadinessUpdateOutcome::Duplicate => ConsumeOutcome::Duplicate,
            PaymentReadinessUpdateOutcome::Stale => ConsumeOutcome::Stale,
        }))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventEnvelope<P> {
    event_id: String,
    #[serde(default)]
    aggregate_id: Option<String>,
    event_type: String,
    schema_version: u32,
    aggregate_version: u64,
    occurred_at_millis: u64,
    correlation_id: String,
    #[serde(default)]
    causation_id: Option<String>,
    payload: P,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogPayload {
    auction_id: String,
    lot_id: String,
    seller_id: String,
    start_price_minor: u64,
    currency_code: String,
    bid_step_minor: u64,
    closing_policy: ClosingPolicyPayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClosingPolicyPayload {
    scheduled_end_millis: Option<u64>,
    inactivity_millis: Option<u64>,
    hard_deadline_millis: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PaymentPayload {
    #[serde(default)]
    instrument_id: Option<String>,
    user_id: String,
}

fn validate_envelope<P>(envelope: &EventEnvelope<P>) -> Result<(), ConsumerError> {
    if envelope.event_id.trim().is_empty()
        || envelope.correlation_id.trim().is_empty()
        || envelope.aggregate_version == 0
        || envelope
            .aggregate_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        || envelope
            .causation_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
    {
        return Err(ConsumerError::Malformed);
    }
    Ok(())
}

fn fingerprint(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut result, "{byte:02x}").expect("writing into String cannot fail");
    }
    result
}

fn observe(outcome: ConsumeOutcome) -> ConsumeOutcome {
    record_consumer(match outcome {
        ConsumeOutcome::Applied => ConsumerMetricOutcome::Applied,
        ConsumeOutcome::Duplicate => ConsumerMetricOutcome::Duplicate,
        ConsumeOutcome::Stale => ConsumerMetricOutcome::Stale,
        ConsumeOutcome::Quarantined => ConsumerMetricOutcome::Quarantined,
    });
    outcome
}

fn map_inbox_error(error: InboxError) -> ConsumerError {
    match error {
        InboxError::Unavailable => ConsumerError::Unavailable,
        InboxError::Internal => ConsumerError::Internal,
    }
}

fn map_port_error(error: PortError) -> ConsumerError {
    match error {
        PortError::Unavailable => ConsumerError::Unavailable,
        PortError::InvalidInput => ConsumerError::Malformed,
        PortError::Internal => ConsumerError::Internal,
    }
}

fn map_application_error(error: ApplicationError) -> ConsumerError {
    match error {
        ApplicationError::DependencyUnavailable { .. } | ApplicationError::Contention => {
            ConsumerError::Unavailable
        }
        ApplicationError::InvalidArgument { .. } => ConsumerError::Malformed,
        _ => ConsumerError::Rejected,
    }
}
