use prost::Message;

use crate::{
    domain::{AuctionEvent, Money, Timestamp},
    rpc,
};

const SCHEMA_VERSION: u32 = 1;

type ProcessPayload = rpc::auction_process_event::Payload;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedProcessEvent {
    pub event_id: String,
    pub aggregate_id: String,
    pub aggregate_version: u64,
    pub event_type: &'static str,
    pub occurred_at: Timestamp,
    pub payload: Vec<u8>,
    pub correlation_id: String,
    pub causation_id: String,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProcessEventCodecError {
    #[error("process event metadata is invalid")]
    InvalidMetadata,
    #[error("unsupported process event schema version {0}")]
    UnsupportedSchema(u32),
    #[error("process event payload is malformed")]
    Malformed,
}

pub fn encode_process_event(
    event_id: impl Into<String>,
    auction_id: &str,
    aggregate_version: u64,
    event: &AuctionEvent,
    occurred_at: Timestamp,
    correlation_id: &str,
    causation_id: &str,
) -> Result<Option<EncodedProcessEvent>, ProcessEventCodecError> {
    let event_id = event_id.into();
    validate_metadata(
        &event_id,
        auction_id,
        aggregate_version,
        correlation_id,
        causation_id,
    )?;
    let Some((event_type, payload)) = process_payload(event) else {
        return Ok(None);
    };
    let envelope = rpc::AuctionProcessEvent {
        event_id: event_id.clone(),
        aggregate_id: auction_id.to_owned(),
        aggregate_version,
        schema_version: SCHEMA_VERSION,
        occurred_at: Some(timestamp_to_proto(occurred_at)?),
        correlation_id: correlation_id.to_owned(),
        causation_id: causation_id.to_owned(),
        payload: Some(payload),
    };

    Ok(Some(EncodedProcessEvent {
        event_id,
        aggregate_id: auction_id.to_owned(),
        aggregate_version,
        event_type,
        occurred_at,
        payload: envelope.encode_to_vec(),
        correlation_id: correlation_id.to_owned(),
        causation_id: causation_id.to_owned(),
    }))
}

pub fn decode_process_event(
    bytes: &[u8],
) -> Result<rpc::AuctionProcessEvent, ProcessEventCodecError> {
    let event =
        rpc::AuctionProcessEvent::decode(bytes).map_err(|_| ProcessEventCodecError::Malformed)?;
    if event.schema_version != SCHEMA_VERSION {
        return Err(ProcessEventCodecError::UnsupportedSchema(
            event.schema_version,
        ));
    }
    validate_metadata(
        &event.event_id,
        &event.aggregate_id,
        event.aggregate_version,
        &event.correlation_id,
        &event.causation_id,
    )?;
    if event.occurred_at.is_none() || event.payload.is_none() {
        return Err(ProcessEventCodecError::Malformed);
    }
    Ok(event)
}

fn process_payload(event: &AuctionEvent) -> Option<(&'static str, ProcessPayload)> {
    match event {
        AuctionEvent::BidderOutbid { bidder_id, bid_id } => Some((
            "BidderOutbid",
            ProcessPayload::BidderOutbid(rpc::ProcessBidderOutbid {
                recipient_id: bidder_id.to_string(),
                bid_id: bid_id.to_string(),
            }),
        )),
        AuctionEvent::SettlementCandidateSelected {
            bidder_id,
            amount,
            round,
        } => Some((
            "SettlementCandidateSelected",
            ProcessPayload::SettlementCandidateSelected(rpc::ProcessSettlementCandidateSelected {
                candidate_id: bidder_id.to_string(),
                settlement_round: *round,
                amount: Some(money_to_proto(amount)),
            }),
        )),
        AuctionEvent::Unsold => Some((
            "AuctionUnsold",
            ProcessPayload::AuctionUnsold(rpc::ProcessAuctionUnsold {}),
        )),
        AuctionEvent::WinnerConfirmed {
            bidder_id,
            payment_id,
        } => Some((
            "WinnerConfirmed",
            ProcessPayload::WinnerConfirmed(rpc::ProcessWinnerConfirmed {
                candidate_id: bidder_id.to_string(),
                payment_id: payment_id.to_string(),
            }),
        )),
        AuctionEvent::Cancelled => Some((
            "AuctionCancelled",
            ProcessPayload::AuctionCancelled(rpc::ProcessAuctionCancelled {}),
        )),
        _ => None,
    }
}

fn validate_metadata(
    event_id: &str,
    aggregate_id: &str,
    aggregate_version: u64,
    correlation_id: &str,
    causation_id: &str,
) -> Result<(), ProcessEventCodecError> {
    if event_id.trim().is_empty()
        || aggregate_id.trim().is_empty()
        || aggregate_version == 0
        || correlation_id.trim().is_empty()
        || causation_id.trim().is_empty()
    {
        return Err(ProcessEventCodecError::InvalidMetadata);
    }
    Ok(())
}

fn timestamp_to_proto(
    timestamp: Timestamp,
) -> Result<prost_types::Timestamp, ProcessEventCodecError> {
    let millis = timestamp.as_millis();
    let seconds = i64::try_from(millis / 1_000).map_err(|_| ProcessEventCodecError::Malformed)?;
    let nanos = i32::try_from((millis % 1_000) * 1_000_000)
        .map_err(|_| ProcessEventCodecError::Malformed)?;
    Ok(prost_types::Timestamp { seconds, nanos })
}

fn money_to_proto(money: &Money) -> rpc::Money {
    rpc::Money {
        minor_units: money.amount_minor(),
        currency_code: money.currency().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BidderId, PaymentId};

    fn encode(event: &AuctionEvent) -> EncodedProcessEvent {
        encode_process_event(
            "event-1",
            "auction-1",
            7,
            event,
            Timestamp::from_millis(1_700_000_000_123),
            "correlation-1",
            "causation-1",
        )
        .expect("event must encode")
        .expect("event must be process-visible")
    }

    #[test]
    fn selected_candidate_round_trips_with_restricted_fields() {
        let encoded = encode(&AuctionEvent::SettlementCandidateSelected {
            bidder_id: BidderId::from("candidate-1"),
            amount: Money::new(1_250, "USD").unwrap(),
            round: 2,
        });
        let decoded = decode_process_event(&encoded.payload).unwrap();

        assert_eq!(encoded.event_type, "SettlementCandidateSelected");
        assert_eq!(decoded.aggregate_version, 7);
        let Some(ProcessPayload::SettlementCandidateSelected(selected)) = decoded.payload else {
            panic!("selected payload expected");
        };
        assert_eq!(selected.candidate_id, "candidate-1");
        assert_eq!(selected.settlement_round, 2);
        assert_eq!(selected.amount.unwrap().minor_units, 1_250);
    }

    #[test]
    fn bidder_outbid_round_trips_without_maximum_or_leader_identity() {
        let encoded = encode(&AuctionEvent::BidderOutbid {
            bidder_id: BidderId::from("bidder@example.test"),
            bid_id: crate::domain::BidId::from("bid-2"),
        });
        let decoded = decode_process_event(&encoded.payload).unwrap();
        assert_eq!(encoded.event_type, "BidderOutbid");
        let Some(ProcessPayload::BidderOutbid(outbid)) = decoded.payload else {
            panic!("outbid payload expected");
        };
        assert_eq!(outbid.recipient_id, "bidder@example.test");
        assert_eq!(outbid.bid_id, "bid-2");
        let debug = format!("{outbid:?}").to_ascii_lowercase();
        assert!(!debug.contains("maximum"));
        assert!(!debug.contains("leader"));
    }

    #[test]
    fn terminal_facts_round_trip_and_unrelated_private_events_are_ignored() {
        for (event, expected) in [
            (AuctionEvent::Unsold, "AuctionUnsold"),
            (AuctionEvent::Cancelled, "AuctionCancelled"),
            (
                AuctionEvent::WinnerConfirmed {
                    bidder_id: BidderId::from("candidate-1"),
                    payment_id: PaymentId::from("payment-1"),
                },
                "WinnerConfirmed",
            ),
        ] {
            let encoded = encode(&event);
            assert_eq!(encoded.event_type, expected);
            assert!(decode_process_event(&encoded.payload).is_ok());
        }

        let unrelated = AuctionEvent::Closed {
            closing_price: Money::new(900, "USD").unwrap(),
        };
        assert!(
            encode_process_event(
                "event-2",
                "auction-1",
                8,
                &unrelated,
                Timestamp::from_millis(1),
                "correlation-1",
                "causation-1",
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn unknown_schema_and_invalid_metadata_are_rejected() {
        let encoded = encode(&AuctionEvent::Unsold);
        let mut envelope = rpc::AuctionProcessEvent::decode(encoded.payload.as_slice()).unwrap();
        envelope.schema_version = 2;
        assert_eq!(
            decode_process_event(&envelope.encode_to_vec()),
            Err(ProcessEventCodecError::UnsupportedSchema(2))
        );

        assert_eq!(
            encode_process_event(
                "",
                "auction-1",
                7,
                &AuctionEvent::Unsold,
                Timestamp::from_millis(1),
                "correlation-1",
                "causation-1",
            ),
            Err(ProcessEventCodecError::InvalidMetadata)
        );
    }
}
