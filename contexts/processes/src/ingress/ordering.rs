use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::{DecodedOwnerEvent, OwnerEvent};
use crate::workflows::{AuctionId, PaymentAttemptId, ProcessError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventSource {
    AuctionPublic,
    AuctionRestricted,
    PaymentsAttempt,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum StreamKey {
    AuctionPublic(AuctionId),
    AuctionRestricted(AuctionId),
    AuctionOutbid(AuctionId, u64),
    PaymentAttempt(PaymentAttemptId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventFingerprint([u8; 32]);

impl EventFingerprint {
    pub fn of(payload: &[u8]) -> Self {
        Self(Sha256::digest(payload).into())
    }
}

impl std::fmt::Debug for EventFingerprint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("EventFingerprint([REDACTED])")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateDecision {
    Accepted,
    Duplicate,
    Stale {
        latest_version: u64,
        received_version: u64,
    },
    Gap {
        expected_version: u64,
        received_version: u64,
    },
    Quarantined,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuarantinedEvent {
    pub event_id: String,
    pub original_fingerprint: EventFingerprint,
    pub changed_fingerprint: EventFingerprint,
}

#[derive(Default)]
pub struct EventGate {
    seen: HashMap<String, EventFingerprint>,
    positions: HashMap<StreamKey, u64>,
    quarantine: Vec<QuarantinedEvent>,
}

impl EventGate {
    /// Computes the ordering decision without changing the durable-acceptance mirror.
    ///
    /// Ingress uses this before calling Temporal and calls [`Self::admit`] only after
    /// the signal is durably accepted. This prevents a transient Temporal failure from
    /// turning the next delivery into a false duplicate.
    pub fn inspect(
        &self,
        source: EventSource,
        event: &DecodedOwnerEvent,
        raw_payload: &[u8],
    ) -> Result<GateDecision, ProcessError> {
        let fingerprint = EventFingerprint::of(raw_payload);
        if let Some(original) = self.seen.get(&event.event_id) {
            return Ok(if original == &fingerprint {
                GateDecision::Duplicate
            } else {
                GateDecision::Quarantined
            });
        }

        let stream = stream_key(source, event)?;
        let latest = self.positions.get(&stream).copied().unwrap_or(0);
        if event.aggregate_version <= latest {
            return Ok(GateDecision::Stale {
                latest_version: latest,
                received_version: event.aggregate_version,
            });
        }
        let expected = latest.saturating_add(1);
        if event.aggregate_version > expected {
            return Ok(GateDecision::Gap {
                expected_version: expected,
                received_version: event.aggregate_version,
            });
        }
        Ok(GateDecision::Accepted)
    }

    pub fn admit(
        &mut self,
        source: EventSource,
        event: &DecodedOwnerEvent,
        raw_payload: &[u8],
    ) -> Result<GateDecision, ProcessError> {
        let decision = self.inspect(source, event, raw_payload)?;
        let fingerprint = EventFingerprint::of(raw_payload);
        if let Some(original) = self.seen.get(&event.event_id) {
            if original == &fingerprint {
                return Ok(GateDecision::Duplicate);
            }
            self.quarantine.push(QuarantinedEvent {
                event_id: event.event_id.clone(),
                original_fingerprint: *original,
                changed_fingerprint: fingerprint,
            });
            return Ok(GateDecision::Quarantined);
        }

        let stream = stream_key(source, event)?;
        self.seen.insert(event.event_id.clone(), fingerprint);
        if matches!(decision, GateDecision::Accepted | GateDecision::Gap { .. }) {
            self.positions.insert(stream, event.aggregate_version);
        }
        Ok(decision)
    }

    pub fn quarantined(&self) -> &[QuarantinedEvent] {
        &self.quarantine
    }
}

fn stream_key(source: EventSource, decoded: &DecodedOwnerEvent) -> Result<StreamKey, ProcessError> {
    let event = &decoded.event;
    let auction_id = match event {
        OwnerEvent::AuctionOpened { auction_id, .. }
        | OwnerEvent::AuctionObserved { auction_id }
        | OwnerEvent::AuctionDeadlineChanged { auction_id, .. }
        | OwnerEvent::AuctionClosed { auction_id }
        | OwnerEvent::AuctionCancelled { auction_id }
        | OwnerEvent::AuctionUnsold { auction_id }
        | OwnerEvent::WinnerConfirmed { auction_id }
        | OwnerEvent::BidderOutbid { auction_id, .. }
        | OwnerEvent::SettlementCandidateSelected { auction_id, .. }
        | OwnerEvent::PaymentAttemptStarted { auction_id, .. }
        | OwnerEvent::PaymentRecoveryRequired { auction_id, .. }
        | OwnerEvent::PaymentSucceeded { auction_id, .. }
        | OwnerEvent::PaymentExpired { auction_id, .. } => auction_id.clone(),
    };
    match source {
        EventSource::AuctionRestricted if matches!(event, OwnerEvent::BidderOutbid { .. }) => Ok(
            StreamKey::AuctionOutbid(auction_id, decoded.aggregate_version),
        ),
        EventSource::AuctionPublic if is_auction_public(event) => {
            Ok(StreamKey::AuctionPublic(auction_id))
        }
        EventSource::AuctionRestricted if is_auction_restricted(event) => {
            Ok(StreamKey::AuctionRestricted(auction_id))
        }
        EventSource::PaymentsAttempt => payment_attempt_id(event)
            .cloned()
            .map(StreamKey::PaymentAttempt)
            .ok_or(ProcessError::InvariantViolation),
        EventSource::AuctionPublic | EventSource::AuctionRestricted => {
            Err(ProcessError::InvariantViolation)
        }
    }
}

fn is_auction_public(event: &OwnerEvent) -> bool {
    matches!(
        event,
        OwnerEvent::AuctionOpened { .. }
            | OwnerEvent::AuctionObserved { .. }
            | OwnerEvent::AuctionDeadlineChanged { .. }
            | OwnerEvent::AuctionClosed { .. }
            | OwnerEvent::AuctionCancelled { .. }
            | OwnerEvent::AuctionUnsold { .. }
            | OwnerEvent::WinnerConfirmed { .. }
    )
}

fn is_auction_restricted(event: &OwnerEvent) -> bool {
    matches!(
        event,
        OwnerEvent::SettlementCandidateSelected { .. }
            | OwnerEvent::BidderOutbid { .. }
            | OwnerEvent::AuctionCancelled { .. }
            | OwnerEvent::AuctionUnsold { .. }
            | OwnerEvent::WinnerConfirmed { .. }
    )
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
