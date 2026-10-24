use gavel_auction::rpc::{
    self as auction_rpc, AuctionProcessEvent, AuctionPublicEvent, auction_process_event,
    auction_public_event,
};
use gavel_processes::ingress::{
    EventDecodeError, OwnerEvent, decode_auction_process, decode_auction_public,
    decode_payment_event,
};
use prost::Message;

fn timestamp(seconds: i64) -> prost_types::Timestamp {
    prost_types::Timestamp { seconds, nanos: 0 }
}

fn public_event(schema_version: u32) -> AuctionPublicEvent {
    AuctionPublicEvent {
        event_id: "auction-event-1".to_owned(),
        aggregate_id: "auction-1".to_owned(),
        aggregate_version: 2,
        schema_version,
        occurred_at: Some(timestamp(1_700_000_000)),
        correlation_id: "correlation-1".to_owned(),
        causation_id: "command-1".to_owned(),
        payload: Some(auction_public_event::Payload::AuctionOpened(
            auction_rpc::AuctionOpenedEvent {
                ends_at: Some(timestamp(1_700_000_600)),
            },
        )),
    }
}

fn process_event(schema_version: u32) -> AuctionProcessEvent {
    AuctionProcessEvent {
        event_id: "auction-process-event-1".to_owned(),
        aggregate_id: "auction-1".to_owned(),
        aggregate_version: 3,
        schema_version,
        occurred_at: Some(timestamp(1_700_000_100)),
        correlation_id: "correlation-1".to_owned(),
        causation_id: "auction-event-1".to_owned(),
        payload: Some(auction_process_event::Payload::SettlementCandidateSelected(
            auction_rpc::ProcessSettlementCandidateSelected {
                candidate_id: "user-1".to_owned(),
                settlement_round: 1,
                amount: Some(auction_rpc::Money {
                    minor_units: 100,
                    currency_code: "USD".to_owned(),
                }),
            },
        )),
    }
}

fn money() -> auction_rpc::Money {
    auction_rpc::Money {
        minor_units: 100,
        currency_code: "USD".to_owned(),
    }
}

#[test]
fn decodes_supported_auction_public_and_restricted_versions() {
    let public = decode_auction_public(&public_event(1).encode_to_vec()).unwrap();
    assert!(matches!(
        public.event,
        OwnerEvent::AuctionOpened {
            ends_at_millis: 1_700_000_600_000,
            ..
        }
    ));
    assert_eq!(public.aggregate_version, 2);
    assert_eq!(public.causation_id.as_deref(), Some("command-1"));

    let process = decode_auction_process(&process_event(1).encode_to_vec()).unwrap();
    assert!(matches!(
        process.event,
        OwnerEvent::SettlementCandidateSelected { round, amount, .. }
            if round.get() == 1 && amount.minor_units == 100
    ));
}

#[test]
fn validates_every_registered_auction_public_envelope() {
    let payloads = vec![
        auction_public_event::Payload::AuctionOpened(auction_rpc::AuctionOpenedEvent {
            ends_at: Some(timestamp(1_700_000_600)),
        }),
        auction_public_event::Payload::BidRecorded(auction_rpc::BidRecordedEvent {
            bid_id: "bid-1".to_owned(),
            bidder_alias: "bidder-7".to_owned(),
            resulting_price: Some(money()),
            recorded_at: Some(timestamp(1_700_000_010)),
        }),
        auction_public_event::Payload::PriceChanged(auction_rpc::PriceChangedEvent {
            price: Some(money()),
            ends_at: Some(timestamp(1_700_000_600)),
        }),
        auction_public_event::Payload::DeadlineExtended(auction_rpc::DeadlineExtendedEvent {
            ends_at: Some(timestamp(1_700_000_700)),
        }),
        auction_public_event::Payload::AuctionRescheduled(auction_rpc::AuctionRescheduledEvent {
            ends_at: Some(timestamp(1_700_000_800)),
        }),
        auction_public_event::Payload::AuctionClosed(auction_rpc::AuctionClosedEvent {
            closing_price: Some(money()),
        }),
        auction_public_event::Payload::AuctionCancelled(auction_rpc::AuctionCancelledEvent {}),
        auction_public_event::Payload::AuctionUnsold(auction_rpc::AuctionUnsoldEvent {}),
        auction_public_event::Payload::WinnerConfirmed(auction_rpc::WinnerConfirmedEvent {}),
    ];
    for (index, payload) in payloads.into_iter().enumerate() {
        let mut envelope = public_event(1);
        envelope.event_id = format!("public-event-{index}");
        envelope.aggregate_version = index as u64 + 1;
        envelope.payload = Some(payload);
        decode_auction_public(&envelope.encode_to_vec()).unwrap();
    }
}

#[test]
fn validates_every_registered_auction_restricted_envelope() {
    let payloads = vec![
        auction_process_event::Payload::SettlementCandidateSelected(
            auction_rpc::ProcessSettlementCandidateSelected {
                candidate_id: "candidate-1".to_owned(),
                settlement_round: 1,
                amount: Some(money()),
            },
        ),
        auction_process_event::Payload::BidderOutbid(auction_rpc::ProcessBidderOutbid {
            recipient_id: "candidate-1".to_owned(),
            bid_id: "bid-1".to_owned(),
        }),
        auction_process_event::Payload::AuctionUnsold(auction_rpc::ProcessAuctionUnsold {}),
        auction_process_event::Payload::WinnerConfirmed(auction_rpc::ProcessWinnerConfirmed {
            candidate_id: "candidate-1".to_owned(),
            payment_id: "payment-1".to_owned(),
        }),
        auction_process_event::Payload::AuctionCancelled(auction_rpc::ProcessAuctionCancelled {}),
    ];
    for (index, payload) in payloads.into_iter().enumerate() {
        let mut envelope = process_event(1);
        envelope.event_id = format!("restricted-event-{index}");
        envelope.aggregate_version = index as u64 + 1;
        envelope.payload = Some(payload);
        decode_auction_process(&envelope.encode_to_vec()).unwrap();
    }
}

#[test]
fn decodes_outbid_only_from_restricted_envelope_and_validates_private_ids() {
    let mut envelope = process_event(1);
    envelope.payload = Some(auction_process_event::Payload::BidderOutbid(
        auction_rpc::ProcessBidderOutbid {
            recipient_id: "candidate-1".to_owned(),
            bid_id: "bid-1".to_owned(),
        },
    ));
    let decoded = decode_auction_process(&envelope.encode_to_vec()).unwrap();
    assert!(matches!(
        decoded.event,
        OwnerEvent::BidderOutbid {
            recipient_id,
            bid_id,
            ..
        } if recipient_id.as_str() == "candidate-1" && bid_id.as_str() == "bid-1"
    ));

    if let Some(auction_process_event::Payload::BidderOutbid(value)) = envelope.payload.as_mut() {
        value.recipient_id.clear();
    }
    assert_eq!(
        decode_auction_process(&envelope.encode_to_vec()).unwrap_err(),
        EventDecodeError::InvalidField
    );
}

#[test]
fn decodes_supported_payment_attempt_fixture() {
    let fixtures: &[&[u8]] = &[
        include_bytes!("../../payments/tests/fixtures/payment_attempt_started_v1.json"),
        include_bytes!("../../payments/tests/fixtures/payment_recovery_required_v1.json"),
        include_bytes!("../../payments/tests/fixtures/payment_succeeded_v1.json"),
        include_bytes!("../../payments/tests/fixtures/payment_expired_v1.json"),
    ];
    let events = fixtures
        .iter()
        .map(|fixture| decode_payment_event(fixture).unwrap().event)
        .collect::<Vec<_>>();
    assert!(matches!(
        events[0],
        OwnerEvent::PaymentAttemptStarted { .. }
    ));
    assert!(matches!(
        events[1],
        OwnerEvent::PaymentRecoveryRequired { .. }
    ));
    assert!(matches!(events[2], OwnerEvent::PaymentSucceeded { .. }));
    assert!(matches!(events[3], OwnerEvent::PaymentExpired { .. }));
}

#[test]
fn rejects_unknown_versions_malformed_payloads_and_event_types() {
    assert_eq!(
        decode_auction_public(&public_event(2).encode_to_vec()).unwrap_err(),
        EventDecodeError::UnsupportedSchemaVersion(2)
    );
    assert_eq!(
        decode_auction_process(&process_event(0).encode_to_vec()).unwrap_err(),
        EventDecodeError::UnsupportedSchemaVersion(0)
    );
    assert_eq!(
        decode_auction_public(&[0xff, 0x01]).unwrap_err(),
        EventDecodeError::Malformed
    );

    let mut observed = public_event(1);
    observed.payload = Some(auction_public_event::Payload::PriceChanged(
        auction_rpc::PriceChangedEvent {
            price: Some(auction_rpc::Money {
                minor_units: 100,
                currency_code: "USD".to_owned(),
            }),
            ends_at: Some(timestamp(1_700_000_600)),
        },
    ));
    assert!(matches!(
        decode_auction_public(&observed.encode_to_vec())
            .unwrap()
            .event,
        OwnerEvent::AuctionObserved { .. }
    ));

    let mut missing_payload = public_event(1);
    missing_payload.payload = None;
    assert_eq!(
        decode_auction_public(&missing_payload.encode_to_vec()).unwrap_err(),
        EventDecodeError::UnknownEventType
    );

    let payment_unknown = br#"{
        "eventId":"event-1","eventType":"ProviderCharged","schemaVersion":1,
        "aggregateId":"attempt-1","aggregateVersion":1,"occurredAtMillis":1,
        "correlationId":"correlation-1","causationId":"cause-1",
        "payload":{}
    }"#;
    assert_eq!(
        decode_payment_event(payment_unknown).unwrap_err(),
        EventDecodeError::UnknownEventType
    );
    assert_eq!(
        decode_payment_event(b"not-json").unwrap_err(),
        EventDecodeError::Malformed
    );
}

#[test]
fn rejects_secret_fields_before_typed_payment_decode() {
    let fixture = include_bytes!("fixtures/payment_secret_injection.json");
    assert_eq!(
        decode_payment_event(fixture).unwrap_err(),
        EventDecodeError::ForbiddenField("recoveryUrl".to_owned())
    );
}

#[test]
fn rejects_payment_payload_bound_to_a_different_aggregate() {
    let mut payload: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/payment_succeeded_v1.json")).unwrap();
    payload["aggregateId"] = serde_json::Value::String("attempt-other".to_owned());
    assert_eq!(
        decode_payment_event(&serde_json::to_vec(&payload).unwrap()).unwrap_err(),
        EventDecodeError::InvalidField
    );
}
