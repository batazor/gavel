use gavel_auction::rpc::{
    AuctionProcessEvent, AuctionPublicEvent, auction_process_event, auction_public_event,
};
use prost::Message;
use serde::Deserialize;
use serde_json::Value;

use crate::workflows::{AuctionId, CandidateId, Money, PaymentAttemptId, SettlementRound};

const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedOwnerEvent {
    pub event_id: String,
    pub aggregate_version: u64,
    pub occurred_at_millis: u64,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub event: OwnerEvent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnerEvent {
    /// Registry-valid public data that does not drive an owner command or workflow.
    AuctionObserved {
        auction_id: AuctionId,
    },
    AuctionOpened {
        auction_id: AuctionId,
        ends_at_millis: u64,
    },
    AuctionDeadlineChanged {
        auction_id: AuctionId,
        ends_at_millis: u64,
    },
    AuctionClosed {
        auction_id: AuctionId,
    },
    AuctionCancelled {
        auction_id: AuctionId,
    },
    AuctionUnsold {
        auction_id: AuctionId,
    },
    WinnerConfirmed {
        auction_id: AuctionId,
    },
    SettlementCandidateSelected {
        auction_id: AuctionId,
        candidate_id: CandidateId,
        round: SettlementRound,
        amount: Money,
    },
    PaymentAttemptStarted {
        auction_id: AuctionId,
        attempt_id: PaymentAttemptId,
        candidate_id: CandidateId,
        round: SettlementRound,
        amount: Money,
    },
    PaymentRecoveryRequired {
        auction_id: AuctionId,
        attempt_id: PaymentAttemptId,
        round: SettlementRound,
    },
    PaymentSucceeded {
        auction_id: AuctionId,
        attempt_id: PaymentAttemptId,
        round: SettlementRound,
    },
    PaymentExpired {
        auction_id: AuctionId,
        attempt_id: PaymentAttemptId,
        round: SettlementRound,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EventDecodeError {
    #[error("event payload is malformed")]
    Malformed,
    #[error("unsupported schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("event type is not supported")]
    UnknownEventType,
    #[error("required event field is absent")]
    MissingField,
    #[error("event field is invalid")]
    InvalidField,
    #[error("event contains forbidden field {0}")]
    ForbiddenField(String),
}

pub fn decode_auction_public(bytes: &[u8]) -> Result<DecodedOwnerEvent, EventDecodeError> {
    let envelope = AuctionPublicEvent::decode(bytes).map_err(|_| EventDecodeError::Malformed)?;
    validate_envelope(
        &envelope.event_id,
        envelope.schema_version,
        envelope.aggregate_version,
        &envelope.correlation_id,
    )?;
    if envelope.causation_id.is_empty() {
        return Err(EventDecodeError::InvalidField);
    }
    let auction_id =
        AuctionId::parse(envelope.aggregate_id).map_err(|_| EventDecodeError::InvalidField)?;
    let occurred_at_millis = timestamp_millis(envelope.occurred_at.as_ref())?;
    let event = match envelope.payload.ok_or(EventDecodeError::UnknownEventType)? {
        auction_public_event::Payload::AuctionOpened(value) => OwnerEvent::AuctionOpened {
            auction_id,
            ends_at_millis: timestamp_millis(value.ends_at.as_ref())?,
        },
        auction_public_event::Payload::DeadlineExtended(value) => {
            OwnerEvent::AuctionDeadlineChanged {
                auction_id,
                ends_at_millis: timestamp_millis(value.ends_at.as_ref())?,
            }
        }
        auction_public_event::Payload::AuctionRescheduled(value) => {
            OwnerEvent::AuctionDeadlineChanged {
                auction_id,
                ends_at_millis: timestamp_millis(value.ends_at.as_ref())?,
            }
        }
        auction_public_event::Payload::AuctionClosed(value) => {
            let price = value.closing_price.ok_or(EventDecodeError::MissingField)?;
            Money::new(price.minor_units, price.currency_code)
                .map_err(|_| EventDecodeError::InvalidField)?;
            OwnerEvent::AuctionClosed { auction_id }
        }
        auction_public_event::Payload::AuctionCancelled(_) => {
            OwnerEvent::AuctionCancelled { auction_id }
        }
        auction_public_event::Payload::AuctionUnsold(_) => OwnerEvent::AuctionUnsold { auction_id },
        auction_public_event::Payload::WinnerConfirmed(_) => {
            OwnerEvent::WinnerConfirmed { auction_id }
        }
        auction_public_event::Payload::BidRecorded(value) => {
            validate_public_bid(&value)?;
            OwnerEvent::AuctionObserved { auction_id }
        }
        auction_public_event::Payload::PriceChanged(value) => {
            validate_public_price(&value)?;
            OwnerEvent::AuctionObserved { auction_id }
        }
    };
    Ok(decoded_auction(
        envelope.event_id,
        envelope.aggregate_version,
        occurred_at_millis,
        envelope.correlation_id,
        envelope.causation_id,
        event,
    ))
}

pub fn decode_auction_process(bytes: &[u8]) -> Result<DecodedOwnerEvent, EventDecodeError> {
    let envelope = AuctionProcessEvent::decode(bytes).map_err(|_| EventDecodeError::Malformed)?;
    validate_envelope(
        &envelope.event_id,
        envelope.schema_version,
        envelope.aggregate_version,
        &envelope.correlation_id,
    )?;
    if envelope.causation_id.is_empty() {
        return Err(EventDecodeError::InvalidField);
    }
    let auction_id =
        AuctionId::parse(envelope.aggregate_id).map_err(|_| EventDecodeError::InvalidField)?;
    let occurred_at_millis = timestamp_millis(envelope.occurred_at.as_ref())?;
    let event = match envelope.payload.ok_or(EventDecodeError::UnknownEventType)? {
        auction_process_event::Payload::SettlementCandidateSelected(value) => {
            let amount = value.amount.ok_or(EventDecodeError::MissingField)?;
            OwnerEvent::SettlementCandidateSelected {
                auction_id,
                candidate_id: CandidateId::parse(value.candidate_id)
                    .map_err(|_| EventDecodeError::InvalidField)?,
                round: SettlementRound::new(value.settlement_round)
                    .map_err(|_| EventDecodeError::InvalidField)?,
                amount: Money::new(amount.minor_units, amount.currency_code)
                    .map_err(|_| EventDecodeError::InvalidField)?,
            }
        }
        auction_process_event::Payload::AuctionUnsold(_) => {
            OwnerEvent::AuctionUnsold { auction_id }
        }
        auction_process_event::Payload::WinnerConfirmed(value) => {
            if value.candidate_id.is_empty() || value.payment_id.is_empty() {
                return Err(EventDecodeError::InvalidField);
            }
            OwnerEvent::WinnerConfirmed { auction_id }
        }
        auction_process_event::Payload::AuctionCancelled(_) => {
            OwnerEvent::AuctionCancelled { auction_id }
        }
    };
    Ok(decoded_auction(
        envelope.event_id,
        envelope.aggregate_version,
        occurred_at_millis,
        envelope.correlation_id,
        envelope.causation_id,
        event,
    ))
}

fn decoded_auction(
    event_id: String,
    aggregate_version: u64,
    occurred_at_millis: u64,
    correlation_id: String,
    causation_id: String,
    event: OwnerEvent,
) -> DecodedOwnerEvent {
    DecodedOwnerEvent {
        event_id,
        aggregate_version,
        occurred_at_millis,
        correlation_id,
        causation_id: (!causation_id.is_empty()).then_some(causation_id),
        event,
    }
}

pub fn decode_payment_event(bytes: &[u8]) -> Result<DecodedOwnerEvent, EventDecodeError> {
    let raw: Value = serde_json::from_slice(bytes).map_err(|_| EventDecodeError::Malformed)?;
    reject_forbidden_fields(&raw)?;
    let envelope: PaymentEnvelope =
        serde_json::from_value(raw).map_err(|_| EventDecodeError::Malformed)?;
    validate_envelope(
        &envelope.event_id,
        envelope.schema_version,
        envelope.aggregate_version,
        &envelope.correlation_id,
    )?;
    if envelope.occurred_at_millis == 0
        || envelope.aggregate_id.is_empty()
        || envelope.causation_id.is_empty()
    {
        return Err(EventDecodeError::InvalidField);
    }
    let payload = envelope.payload;
    let event = match envelope.event_type.as_str() {
        "PaymentAttemptStarted" => {
            let value: PaymentAttemptStarted =
                serde_json::from_value(payload).map_err(|_| EventDecodeError::Malformed)?;
            OwnerEvent::PaymentAttemptStarted {
                auction_id: parse_auction(value.auction_id)?,
                attempt_id: parse_attempt(value.payment_attempt_id)?,
                candidate_id: CandidateId::parse(value.candidate_id)
                    .map_err(|_| EventDecodeError::InvalidField)?,
                round: parse_round(value.settlement_round)?,
                amount: Money::new(value.amount.minor_units, value.amount.currency_code)
                    .map_err(|_| EventDecodeError::InvalidField)?,
            }
        }
        "PaymentRecoveryRequired" => {
            let value: PaymentAttemptReference =
                serde_json::from_value(payload).map_err(|_| EventDecodeError::Malformed)?;
            payment_reference(value, PaymentReferenceKind::Recovery)?
        }
        "PaymentSucceeded" => {
            let value: PaymentAttemptReference =
                serde_json::from_value(payload).map_err(|_| EventDecodeError::Malformed)?;
            payment_reference(value, PaymentReferenceKind::Succeeded)?
        }
        "PaymentExpired" => {
            let value: PaymentAttemptReference =
                serde_json::from_value(payload).map_err(|_| EventDecodeError::Malformed)?;
            payment_reference(value, PaymentReferenceKind::Expired)?
        }
        _ => return Err(EventDecodeError::UnknownEventType),
    };
    if payment_attempt_id(&event).map(PaymentAttemptId::as_str)
        != Some(envelope.aggregate_id.as_str())
    {
        return Err(EventDecodeError::InvalidField);
    }
    Ok(DecodedOwnerEvent {
        event_id: envelope.event_id,
        aggregate_version: envelope.aggregate_version,
        occurred_at_millis: envelope.occurred_at_millis,
        correlation_id: envelope.correlation_id,
        causation_id: Some(envelope.causation_id),
        event,
    })
}

fn payment_attempt_id(event: &OwnerEvent) -> Option<&PaymentAttemptId> {
    match event {
        OwnerEvent::PaymentAttemptStarted { attempt_id, .. }
        | OwnerEvent::PaymentRecoveryRequired { attempt_id, .. }
        | OwnerEvent::PaymentSucceeded { attempt_id, .. }
        | OwnerEvent::PaymentExpired { attempt_id, .. } => Some(attempt_id),
        _ => None,
    }
}

fn validate_public_bid(
    value: &gavel_auction::rpc::BidRecordedEvent,
) -> Result<(), EventDecodeError> {
    if value.bid_id.is_empty() || value.bidder_alias.is_empty() {
        return Err(EventDecodeError::InvalidField);
    }
    let price = value
        .resulting_price
        .as_ref()
        .ok_or(EventDecodeError::MissingField)?;
    Money::new(price.minor_units, price.currency_code.clone())
        .map_err(|_| EventDecodeError::InvalidField)?;
    timestamp_millis(value.recorded_at.as_ref())?;
    Ok(())
}

fn validate_public_price(
    value: &gavel_auction::rpc::PriceChangedEvent,
) -> Result<(), EventDecodeError> {
    let price = value.price.as_ref().ok_or(EventDecodeError::MissingField)?;
    Money::new(price.minor_units, price.currency_code.clone())
        .map_err(|_| EventDecodeError::InvalidField)?;
    timestamp_millis(value.ends_at.as_ref())?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PaymentEnvelope {
    event_id: String,
    aggregate_id: String,
    event_type: String,
    schema_version: u32,
    aggregate_version: u64,
    occurred_at_millis: u64,
    correlation_id: String,
    causation_id: String,
    payload: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PaymentAttemptStarted {
    payment_attempt_id: String,
    auction_id: String,
    settlement_round: u32,
    candidate_id: String,
    amount: PaymentMoney,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PaymentMoney {
    minor_units: u64,
    currency_code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PaymentAttemptReference {
    payment_attempt_id: String,
    auction_id: String,
    settlement_round: u32,
}

#[derive(Clone, Copy)]
enum PaymentReferenceKind {
    Recovery,
    Succeeded,
    Expired,
}

fn payment_reference(
    value: PaymentAttemptReference,
    kind: PaymentReferenceKind,
) -> Result<OwnerEvent, EventDecodeError> {
    let auction_id = parse_auction(value.auction_id)?;
    let attempt_id = parse_attempt(value.payment_attempt_id)?;
    let round = parse_round(value.settlement_round)?;
    Ok(match kind {
        PaymentReferenceKind::Recovery => OwnerEvent::PaymentRecoveryRequired {
            auction_id,
            attempt_id,
            round,
        },
        PaymentReferenceKind::Succeeded => OwnerEvent::PaymentSucceeded {
            auction_id,
            attempt_id,
            round,
        },
        PaymentReferenceKind::Expired => OwnerEvent::PaymentExpired {
            auction_id,
            attempt_id,
            round,
        },
    })
}

fn validate_envelope(
    event_id: &str,
    schema_version: u32,
    aggregate_version: u64,
    correlation_id: &str,
) -> Result<(), EventDecodeError> {
    if schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(EventDecodeError::UnsupportedSchemaVersion(schema_version));
    }
    if event_id.is_empty() || aggregate_version == 0 || correlation_id.is_empty() {
        return Err(EventDecodeError::InvalidField);
    }
    Ok(())
}

fn timestamp_millis(value: Option<&prost_types::Timestamp>) -> Result<u64, EventDecodeError> {
    let value = value.ok_or(EventDecodeError::MissingField)?;
    if value.seconds < 0 || !(0..1_000_000_000).contains(&value.nanos) {
        return Err(EventDecodeError::InvalidField);
    }
    u64::try_from(value.seconds)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000))
        .and_then(|millis| millis.checked_add(u64::try_from(value.nanos / 1_000_000).ok()?))
        .ok_or(EventDecodeError::InvalidField)
}

fn parse_auction(value: String) -> Result<AuctionId, EventDecodeError> {
    AuctionId::parse(value).map_err(|_| EventDecodeError::InvalidField)
}

fn parse_attempt(value: String) -> Result<PaymentAttemptId, EventDecodeError> {
    PaymentAttemptId::parse(value).map_err(|_| EventDecodeError::InvalidField)
}

fn parse_round(value: u32) -> Result<SettlementRound, EventDecodeError> {
    SettlementRound::new(value).map_err(|_| EventDecodeError::InvalidField)
}

fn reject_forbidden_fields(value: &Value) -> Result<(), EventDecodeError> {
    const FORBIDDEN: &[&str] = &[
        "providerref",
        "customerref",
        "instrumentref",
        "providerobject",
        "storagekey",
        "token",
        "credential",
        "maximumamount",
        "secretmaximum",
        "recoveryurl",
        "signature",
        "email",
    ];
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let normalized = key.replace('_', "").to_ascii_lowercase();
                if FORBIDDEN.contains(&normalized.as_str()) {
                    return Err(EventDecodeError::ForbiddenField(key.clone()));
                }
                reject_forbidden_fields(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_forbidden_fields(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
