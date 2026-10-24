use gavel_processes::{
    ingress::{DecodedOwnerEvent, EventGate, EventSource, GateDecision, OwnerEvent},
    workflows::{AuctionId, BidId, CandidateId, PaymentAttemptId, SettlementRound},
};

fn auction_event(event_id: &str, version: u64) -> DecodedOwnerEvent {
    DecodedOwnerEvent {
        event_id: event_id.to_owned(),
        aggregate_version: version,
        occurred_at_millis: 1,
        correlation_id: "correlation-1".to_owned(),
        causation_id: None,
        event: OwnerEvent::AuctionClosed {
            auction_id: AuctionId::parse("auction-1").unwrap(),
        },
    }
}

fn payment_event(event_id: &str, version: u64) -> DecodedOwnerEvent {
    DecodedOwnerEvent {
        event_id: event_id.to_owned(),
        aggregate_version: version,
        occurred_at_millis: 1,
        correlation_id: "correlation-1".to_owned(),
        causation_id: None,
        event: OwnerEvent::PaymentSucceeded {
            auction_id: AuctionId::parse("auction-1").unwrap(),
            attempt_id: PaymentAttemptId::parse("attempt-1").unwrap(),
            round: SettlementRound::new(1).unwrap(),
        },
    }
}

fn outbid_event(event_id: &str, version: u64) -> DecodedOwnerEvent {
    DecodedOwnerEvent {
        event_id: event_id.to_owned(),
        aggregate_version: version,
        occurred_at_millis: 1,
        correlation_id: "correlation-1".to_owned(),
        causation_id: None,
        event: OwnerEvent::BidderOutbid {
            auction_id: AuctionId::parse("auction-1").unwrap(),
            recipient_id: CandidateId::parse("candidate-1").unwrap(),
            bid_id: BidId::parse(format!("bid-{version}")).unwrap(),
        },
    }
}

#[test]
fn exact_redelivery_is_duplicate_and_changed_redelivery_is_quarantined() {
    let mut gate = EventGate::default();
    let event = auction_event("event-1", 1);
    assert_eq!(
        gate.admit(EventSource::AuctionPublic, &event, b"payload-a")
            .unwrap(),
        GateDecision::Accepted
    );
    assert_eq!(
        gate.admit(EventSource::AuctionPublic, &event, b"payload-a")
            .unwrap(),
        GateDecision::Duplicate
    );
    assert_eq!(
        gate.admit(EventSource::AuctionPublic, &event, b"payload-b")
            .unwrap(),
        GateDecision::Quarantined
    );
    assert_eq!(gate.quarantined().len(), 1);
    assert_eq!(gate.quarantined()[0].event_id, "event-1");
    assert!(!format!("{:?}", gate.quarantined()[0]).contains("payload"));
}

#[test]
fn stale_out_of_order_and_gap_versions_are_deterministic() {
    let mut gate = EventGate::default();
    assert_eq!(
        gate.admit(
            EventSource::AuctionPublic,
            &auction_event("event-1", 1),
            b"one"
        )
        .unwrap(),
        GateDecision::Accepted
    );
    assert_eq!(
        gate.admit(
            EventSource::AuctionPublic,
            &auction_event("event-2", 2),
            b"two"
        )
        .unwrap(),
        GateDecision::Accepted
    );
    assert_eq!(
        gate.admit(
            EventSource::AuctionPublic,
            &auction_event("event-stale", 1),
            b"stale"
        )
        .unwrap(),
        GateDecision::Stale {
            latest_version: 2,
            received_version: 1
        }
    );
    assert_eq!(
        gate.admit(
            EventSource::AuctionPublic,
            &auction_event("event-gap", 4),
            b"gap"
        )
        .unwrap(),
        GateDecision::Gap {
            expected_version: 3,
            received_version: 4
        }
    );
    assert_eq!(
        gate.admit(
            EventSource::AuctionPublic,
            &auction_event("event-late", 3),
            b"late"
        )
        .unwrap(),
        GateDecision::Stale {
            latest_version: 4,
            received_version: 3
        }
    );
}

#[test]
fn auction_streams_and_payment_attempt_versions_are_independent() {
    let mut gate = EventGate::default();
    assert_eq!(
        gate.admit(
            EventSource::AuctionPublic,
            &auction_event("public-1", 1),
            b"public"
        )
        .unwrap(),
        GateDecision::Accepted
    );
    let restricted = DecodedOwnerEvent {
        event_id: "restricted-1".to_owned(),
        event: OwnerEvent::AuctionUnsold {
            auction_id: AuctionId::parse("auction-1").unwrap(),
        },
        ..auction_event("ignored", 1)
    };
    assert_eq!(
        gate.admit(EventSource::AuctionRestricted, &restricted, b"restricted")
            .unwrap(),
        GateDecision::Accepted
    );
    assert_eq!(
        gate.admit(
            EventSource::PaymentsAttempt,
            &payment_event("payment-1", 1),
            b"payment"
        )
        .unwrap(),
        GateDecision::Accepted
    );
}

#[test]
fn outbid_facts_remain_deliverable_when_distinct_versions_arrive_out_of_order() {
    let mut gate = EventGate::default();
    assert_eq!(
        gate.admit(
            EventSource::AuctionRestricted,
            &outbid_event("outbid-7", 7),
            b"outbid-seven"
        )
        .unwrap(),
        GateDecision::Gap {
            expected_version: 1,
            received_version: 7
        }
    );
    assert_eq!(
        gate.admit(
            EventSource::AuctionRestricted,
            &outbid_event("outbid-5", 5),
            b"outbid-five"
        )
        .unwrap(),
        GateDecision::Gap {
            expected_version: 1,
            received_version: 5
        }
    );
}
