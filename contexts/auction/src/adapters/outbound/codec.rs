use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{
    Auction, AuctionEvent, AuctionId, AuctionSnapshot, AuctionStatus, BidId, BidSnapshot,
    BidderAlias, BidderId, ClosingPolicy, DurationMillis, LotId, Money, PaymentId, SellerId,
    SettlementCandidateSnapshot, Timestamp,
};

#[derive(Clone, Debug, PartialEq)]
pub struct StoredPrivateEvent {
    pub event_type: String,
    pub schema_version: u32,
    pub payload: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredSnapshot {
    pub schema_version: u32,
    pub payload: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("unsupported private event {event_type} schema version {schema_version}")]
    UnsupportedEventVersion {
        event_type: String,
        schema_version: u32,
    },
    #[error("private event type does not match payload")]
    EventTypeMismatch,
    #[error("private event payload is invalid")]
    InvalidPayload,
}

pub struct PrivateEventCodec;

impl PrivateEventCodec {
    pub fn encode(event: &AuctionEvent) -> Result<StoredPrivateEvent, CodecError> {
        let wire = WireEvent::from(event);
        Ok(StoredPrivateEvent {
            event_type: wire.event_type().to_owned(),
            schema_version: 1,
            payload: serde_json::to_value(wire).map_err(|_| CodecError::InvalidPayload)?,
        })
    }

    pub fn decode(stored: StoredPrivateEvent) -> Result<AuctionEvent, CodecError> {
        if stored.schema_version == 0 && stored.event_type == "AuctionOpened" {
            return decode_legacy_opened(stored.payload);
        }
        if stored.schema_version != 1 {
            return Err(CodecError::UnsupportedEventVersion {
                event_type: stored.event_type,
                schema_version: stored.schema_version,
            });
        }
        let wire: WireEvent =
            serde_json::from_value(stored.payload).map_err(|_| CodecError::InvalidPayload)?;
        if wire.event_type() != stored.event_type {
            return Err(CodecError::EventTypeMismatch);
        }
        wire.try_into()
    }
}

pub struct SnapshotCodec;

impl SnapshotCodec {
    pub fn encode(auction: &Auction) -> Result<StoredSnapshot, CodecError> {
        Ok(StoredSnapshot {
            schema_version: 1,
            payload: serde_json::to_value(WireSnapshot::from(auction.snapshot()))
                .map_err(|_| CodecError::InvalidPayload)?,
        })
    }

    /// Unknown or corrupt snapshots are cache misses; callers replay the stream.
    pub fn decode(stored: StoredSnapshot) -> Result<Option<Auction>, CodecError> {
        if stored.schema_version != 1 {
            return Ok(None);
        }
        let wire: WireSnapshot = match serde_json::from_value(stored.payload) {
            Ok(wire) => wire,
            Err(_) => return Ok(None),
        };
        let snapshot = match AuctionSnapshot::try_from(wire) {
            Ok(snapshot) => snapshot,
            Err(_) => return Ok(None),
        };
        Ok(Auction::from_snapshot(snapshot).ok())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WireEvent {
    Opened {
        auction_id: String,
        lot_id: String,
        seller_id: String,
        start_price: WireMoney,
        bid_step_minor: u64,
        closing_policy: WireClosingPolicy,
        opened_at_millis: u64,
        ends_at_millis: u64,
    },
    MaximumBidPlaced {
        bid_id: String,
        bidder_id: String,
        bidder_alias: u64,
        maximum: WireMoney,
        placed_at_millis: u64,
        first_bid_sequence: u64,
    },
    BidderOutbid {
        bidder_id: String,
        bid_id: String,
    },
    DeadlineExtended {
        ends_at_millis: u64,
    },
    Rescheduled {
        scheduled_end_millis: u64,
        ends_at_millis: u64,
    },
    Closed {
        closing_price: WireMoney,
    },
    Cancelled,
    SettlementCandidateSelected {
        bidder_id: String,
        amount: WireMoney,
        round: u32,
    },
    SettlementCandidateForfeited {
        bidder_id: String,
        round: u32,
    },
    WinnerConfirmed {
        bidder_id: String,
        payment_id: String,
    },
    Unsold,
}

impl WireEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Opened { .. } => "AuctionOpened",
            Self::MaximumBidPlaced { .. } => "MaxBidPlaced",
            Self::BidderOutbid { .. } => "BidderOutbid",
            Self::DeadlineExtended { .. } => "DeadlineExtended",
            Self::Rescheduled { .. } => "AuctionRescheduled",
            Self::Closed { .. } => "AuctionClosed",
            Self::Cancelled => "AuctionCancelled",
            Self::SettlementCandidateSelected { .. } => "SettlementCandidateSelected",
            Self::SettlementCandidateForfeited { .. } => "SettlementCandidateForfeited",
            Self::WinnerConfirmed { .. } => "WinnerConfirmed",
            Self::Unsold => "AuctionUnsold",
        }
    }
}

impl From<&AuctionEvent> for WireEvent {
    fn from(event: &AuctionEvent) -> Self {
        match event {
            AuctionEvent::Opened {
                auction_id,
                lot_id,
                seller_id,
                start_price,
                bid_step_minor,
                closing_policy,
                opened_at,
                ends_at,
            } => Self::Opened {
                auction_id: auction_id.to_string(),
                lot_id: lot_id.to_string(),
                seller_id: seller_id.to_string(),
                start_price: start_price.into(),
                bid_step_minor: *bid_step_minor,
                closing_policy: closing_policy.into(),
                opened_at_millis: opened_at.as_millis(),
                ends_at_millis: ends_at.as_millis(),
            },
            AuctionEvent::MaximumBidPlaced {
                bid_id,
                bidder_id,
                bidder_alias,
                maximum,
                placed_at,
                first_bid_sequence,
            } => Self::MaximumBidPlaced {
                bid_id: bid_id.to_string(),
                bidder_id: bidder_id.to_string(),
                bidder_alias: bidder_alias.sequence(),
                maximum: maximum.into(),
                placed_at_millis: placed_at.as_millis(),
                first_bid_sequence: *first_bid_sequence,
            },
            AuctionEvent::BidderOutbid { bidder_id, bid_id } => Self::BidderOutbid {
                bidder_id: bidder_id.to_string(),
                bid_id: bid_id.to_string(),
            },
            AuctionEvent::DeadlineExtended { ends_at } => Self::DeadlineExtended {
                ends_at_millis: ends_at.as_millis(),
            },
            AuctionEvent::Rescheduled {
                scheduled_end,
                ends_at,
            } => Self::Rescheduled {
                scheduled_end_millis: scheduled_end.as_millis(),
                ends_at_millis: ends_at.as_millis(),
            },
            AuctionEvent::Closed { closing_price } => Self::Closed {
                closing_price: closing_price.into(),
            },
            AuctionEvent::Cancelled => Self::Cancelled,
            AuctionEvent::SettlementCandidateSelected {
                bidder_id,
                amount,
                round,
            } => Self::SettlementCandidateSelected {
                bidder_id: bidder_id.to_string(),
                amount: amount.into(),
                round: *round,
            },
            AuctionEvent::SettlementCandidateForfeited { bidder_id, round } => {
                Self::SettlementCandidateForfeited {
                    bidder_id: bidder_id.to_string(),
                    round: *round,
                }
            }
            AuctionEvent::WinnerConfirmed {
                bidder_id,
                payment_id,
            } => Self::WinnerConfirmed {
                bidder_id: bidder_id.to_string(),
                payment_id: payment_id.to_string(),
            },
            AuctionEvent::Unsold => Self::Unsold,
        }
    }
}

impl TryFrom<WireEvent> for AuctionEvent {
    type Error = CodecError;

    fn try_from(event: WireEvent) -> Result<Self, Self::Error> {
        Ok(match event {
            WireEvent::Opened {
                auction_id,
                lot_id,
                seller_id,
                start_price,
                bid_step_minor,
                closing_policy,
                opened_at_millis,
                ends_at_millis,
            } => Self::Opened {
                auction_id: AuctionId::from(auction_id),
                lot_id: LotId::from(lot_id),
                seller_id: SellerId::from(seller_id),
                start_price: start_price.try_into()?,
                bid_step_minor,
                closing_policy: closing_policy.try_into()?,
                opened_at: Timestamp::from_millis(opened_at_millis),
                ends_at: Timestamp::from_millis(ends_at_millis),
            },
            WireEvent::MaximumBidPlaced {
                bid_id,
                bidder_id,
                bidder_alias,
                maximum,
                placed_at_millis,
                first_bid_sequence,
            } => Self::MaximumBidPlaced {
                bid_id: BidId::from(bid_id),
                bidder_id: BidderId::from(bidder_id),
                bidder_alias: BidderAlias::from_sequence(bidder_alias),
                maximum: maximum.try_into()?,
                placed_at: Timestamp::from_millis(placed_at_millis),
                first_bid_sequence,
            },
            WireEvent::BidderOutbid { bidder_id, bid_id } => Self::BidderOutbid {
                bidder_id: BidderId::from(bidder_id),
                bid_id: BidId::from(bid_id),
            },
            WireEvent::DeadlineExtended { ends_at_millis } => Self::DeadlineExtended {
                ends_at: Timestamp::from_millis(ends_at_millis),
            },
            WireEvent::Rescheduled {
                scheduled_end_millis,
                ends_at_millis,
            } => Self::Rescheduled {
                scheduled_end: Timestamp::from_millis(scheduled_end_millis),
                ends_at: Timestamp::from_millis(ends_at_millis),
            },
            WireEvent::Closed { closing_price } => Self::Closed {
                closing_price: closing_price.try_into()?,
            },
            WireEvent::Cancelled => Self::Cancelled,
            WireEvent::SettlementCandidateSelected {
                bidder_id,
                amount,
                round,
            } => Self::SettlementCandidateSelected {
                bidder_id: BidderId::from(bidder_id),
                amount: amount.try_into()?,
                round,
            },
            WireEvent::SettlementCandidateForfeited { bidder_id, round } => {
                Self::SettlementCandidateForfeited {
                    bidder_id: BidderId::from(bidder_id),
                    round,
                }
            }
            WireEvent::WinnerConfirmed {
                bidder_id,
                payment_id,
            } => Self::WinnerConfirmed {
                bidder_id: BidderId::from(bidder_id),
                payment_id: PaymentId::from(payment_id),
            },
            WireEvent::Unsold => Self::Unsold,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WireMoney {
    amount_minor: u64,
    currency_code: String,
}

impl From<&Money> for WireMoney {
    fn from(money: &Money) -> Self {
        Self {
            amount_minor: money.amount_minor(),
            currency_code: money.currency().to_owned(),
        }
    }
}

impl TryFrom<WireMoney> for Money {
    type Error = CodecError;

    fn try_from(money: WireMoney) -> Result<Self, Self::Error> {
        Money::new(money.amount_minor, money.currency_code).map_err(|_| CodecError::InvalidPayload)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WireClosingPolicy {
    scheduled_end_millis: Option<u64>,
    inactivity_millis: Option<u64>,
    hard_deadline_millis: Option<u64>,
}

impl From<&ClosingPolicy> for WireClosingPolicy {
    fn from(policy: &ClosingPolicy) -> Self {
        Self {
            scheduled_end_millis: policy.scheduled_end().map(Timestamp::as_millis),
            inactivity_millis: policy.inactivity().map(DurationMillis::as_millis),
            hard_deadline_millis: policy.hard_deadline().map(Timestamp::as_millis),
        }
    }
}

impl TryFrom<WireClosingPolicy> for ClosingPolicy {
    type Error = CodecError;

    fn try_from(policy: WireClosingPolicy) -> Result<Self, Self::Error> {
        ClosingPolicy::new(
            policy.scheduled_end_millis.map(Timestamp::from_millis),
            policy.inactivity_millis.and_then(DurationMillis::new),
            policy.hard_deadline_millis.map(Timestamp::from_millis),
        )
        .map_err(|_| CodecError::InvalidPayload)
    }
}

#[derive(Deserialize)]
struct LegacyOpenedV0 {
    auction_id: String,
    lot_id: String,
    seller_id: String,
    start_price_minor: u64,
    currency: String,
    bid_step_minor: u64,
    scheduled_end_millis: Option<u64>,
    inactivity_millis: Option<u64>,
    hard_deadline_millis: Option<u64>,
    opened_at_millis: u64,
    ends_at_millis: u64,
}

fn decode_legacy_opened(payload: Value) -> Result<AuctionEvent, CodecError> {
    let legacy: LegacyOpenedV0 =
        serde_json::from_value(payload).map_err(|_| CodecError::InvalidPayload)?;
    Ok(AuctionEvent::Opened {
        auction_id: AuctionId::from(legacy.auction_id),
        lot_id: LotId::from(legacy.lot_id),
        seller_id: SellerId::from(legacy.seller_id),
        start_price: Money::new(legacy.start_price_minor, legacy.currency)
            .map_err(|_| CodecError::InvalidPayload)?,
        bid_step_minor: legacy.bid_step_minor,
        closing_policy: WireClosingPolicy {
            scheduled_end_millis: legacy.scheduled_end_millis,
            inactivity_millis: legacy.inactivity_millis,
            hard_deadline_millis: legacy.hard_deadline_millis,
        }
        .try_into()?,
        opened_at: Timestamp::from_millis(legacy.opened_at_millis),
        ends_at: Timestamp::from_millis(legacy.ends_at_millis),
    })
}

#[derive(Serialize, Deserialize)]
struct WireSnapshot {
    version: u64,
    status: String,
    id: Option<String>,
    lot_id: Option<String>,
    seller_id: Option<String>,
    start_price: Option<WireMoney>,
    bid_step_minor: u64,
    closing_policy: Option<WireClosingPolicy>,
    opened_at_millis: Option<u64>,
    ends_at_millis: Option<u64>,
    bids: Vec<WireBid>,
    next_bid_sequence: u64,
    current_price: Option<WireMoney>,
    closing_price: Option<WireMoney>,
    forfeited: Vec<String>,
    settlement_candidate: Option<WireSettlementCandidate>,
    winner: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct WireBid {
    bidder_id: String,
    maximum: WireMoney,
    alias: u64,
    first_bid_sequence: u64,
    last_placed_at_millis: u64,
}

#[derive(Serialize, Deserialize)]
struct WireSettlementCandidate {
    bidder_id: String,
    amount: WireMoney,
    round: u32,
}

impl From<AuctionSnapshot> for WireSnapshot {
    fn from(snapshot: AuctionSnapshot) -> Self {
        Self {
            version: snapshot.version,
            status: status_name(snapshot.status).to_owned(),
            id: snapshot.id.map(|id| id.to_string()),
            lot_id: snapshot.lot_id.map(|id| id.to_string()),
            seller_id: snapshot.seller_id.map(|id| id.to_string()),
            start_price: snapshot.start_price.as_ref().map(Into::into),
            bid_step_minor: snapshot.bid_step_minor,
            closing_policy: snapshot.closing_policy.as_ref().map(Into::into),
            opened_at_millis: snapshot.opened_at.map(Timestamp::as_millis),
            ends_at_millis: snapshot.ends_at.map(Timestamp::as_millis),
            bids: snapshot
                .bids
                .into_iter()
                .map(|bid| WireBid {
                    bidder_id: bid.bidder_id.to_string(),
                    maximum: (&bid.maximum).into(),
                    alias: bid.alias.sequence(),
                    first_bid_sequence: bid.first_bid_sequence,
                    last_placed_at_millis: bid.last_placed_at.as_millis(),
                })
                .collect(),
            next_bid_sequence: snapshot.next_bid_sequence,
            current_price: snapshot.current_price.as_ref().map(Into::into),
            closing_price: snapshot.closing_price.as_ref().map(Into::into),
            forfeited: snapshot
                .forfeited
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            settlement_candidate: snapshot.settlement_candidate.map(|candidate| {
                WireSettlementCandidate {
                    bidder_id: candidate.bidder_id.to_string(),
                    amount: (&candidate.amount).into(),
                    round: candidate.round,
                }
            }),
            winner: snapshot.winner.map(|id| id.to_string()),
        }
    }
}

impl TryFrom<WireSnapshot> for AuctionSnapshot {
    type Error = CodecError;

    fn try_from(snapshot: WireSnapshot) -> Result<Self, Self::Error> {
        Ok(Self {
            version: snapshot.version,
            status: parse_status(&snapshot.status)?,
            id: snapshot.id.map(AuctionId::from),
            lot_id: snapshot.lot_id.map(LotId::from),
            seller_id: snapshot.seller_id.map(SellerId::from),
            start_price: snapshot.start_price.map(TryInto::try_into).transpose()?,
            bid_step_minor: snapshot.bid_step_minor,
            closing_policy: snapshot.closing_policy.map(TryInto::try_into).transpose()?,
            opened_at: snapshot.opened_at_millis.map(Timestamp::from_millis),
            ends_at: snapshot.ends_at_millis.map(Timestamp::from_millis),
            bids: snapshot
                .bids
                .into_iter()
                .map(|bid| {
                    Ok(BidSnapshot {
                        bidder_id: BidderId::from(bid.bidder_id),
                        maximum: bid.maximum.try_into()?,
                        alias: BidderAlias::from_sequence(bid.alias),
                        first_bid_sequence: bid.first_bid_sequence,
                        last_placed_at: Timestamp::from_millis(bid.last_placed_at_millis),
                    })
                })
                .collect::<Result<_, CodecError>>()?,
            next_bid_sequence: snapshot.next_bid_sequence,
            current_price: snapshot.current_price.map(TryInto::try_into).transpose()?,
            closing_price: snapshot.closing_price.map(TryInto::try_into).transpose()?,
            forfeited: snapshot.forfeited.into_iter().map(BidderId::from).collect(),
            settlement_candidate: snapshot
                .settlement_candidate
                .map(|candidate| {
                    Ok(SettlementCandidateSnapshot {
                        bidder_id: BidderId::from(candidate.bidder_id),
                        amount: candidate.amount.try_into()?,
                        round: candidate.round,
                    })
                })
                .transpose()?,
            winner: snapshot.winner.map(BidderId::from),
        })
    }
}

fn status_name(status: AuctionStatus) -> &'static str {
    match status {
        AuctionStatus::Uninitialized => "UNINITIALIZED",
        AuctionStatus::Open => "OPEN",
        AuctionStatus::Settling => "SETTLING",
        AuctionStatus::Sold => "SOLD",
        AuctionStatus::Unsold => "UNSOLD",
        AuctionStatus::Cancelled => "CANCELLED",
    }
}

fn parse_status(status: &str) -> Result<AuctionStatus, CodecError> {
    match status {
        "UNINITIALIZED" => Ok(AuctionStatus::Uninitialized),
        "OPEN" => Ok(AuctionStatus::Open),
        "SETTLING" => Ok(AuctionStatus::Settling),
        "SOLD" => Ok(AuctionStatus::Sold),
        "UNSOLD" => Ok(AuctionStatus::Unsold),
        "CANCELLED" => Ok(AuctionStatus::Cancelled),
        _ => Err(CodecError::InvalidPayload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AuctionCommand, PublicEvent};

    fn money(amount: u64) -> Money {
        Money::new(amount, "USD").unwrap()
    }

    fn policy() -> ClosingPolicy {
        ClosingPolicy::new(Some(Timestamp::from_millis(1_000)), None, None).unwrap()
    }

    fn all_private_events() -> Vec<AuctionEvent> {
        vec![
            AuctionEvent::Opened {
                auction_id: AuctionId::from("auction-1"),
                lot_id: LotId::from("lot-1"),
                seller_id: SellerId::from("seller-1"),
                start_price: money(100),
                bid_step_minor: 10,
                closing_policy: policy(),
                opened_at: Timestamp::from_millis(100),
                ends_at: Timestamp::from_millis(1_000),
            },
            AuctionEvent::MaximumBidPlaced {
                bid_id: BidId::from("bid-1"),
                bidder_id: BidderId::from("user-1"),
                bidder_alias: BidderAlias::from_sequence(1),
                maximum: money(300),
                placed_at: Timestamp::from_millis(200),
                first_bid_sequence: 1,
            },
            AuctionEvent::BidderOutbid {
                bidder_id: BidderId::from("user-1"),
                bid_id: BidId::from("bid-2"),
            },
            AuctionEvent::DeadlineExtended {
                ends_at: Timestamp::from_millis(1_100),
            },
            AuctionEvent::Rescheduled {
                scheduled_end: Timestamp::from_millis(1_200),
                ends_at: Timestamp::from_millis(1_200),
            },
            AuctionEvent::Closed {
                closing_price: money(100),
            },
            AuctionEvent::Cancelled,
            AuctionEvent::SettlementCandidateSelected {
                bidder_id: BidderId::from("user-1"),
                amount: money(100),
                round: 1,
            },
            AuctionEvent::SettlementCandidateForfeited {
                bidder_id: BidderId::from("user-1"),
                round: 1,
            },
            AuctionEvent::WinnerConfirmed {
                bidder_id: BidderId::from("user-1"),
                payment_id: PaymentId::from("payment-1"),
            },
            AuctionEvent::Unsold,
        ]
    }

    #[test]
    fn every_private_event_round_trips() {
        for event in all_private_events() {
            let stored = PrivateEventCodec::encode(&event).unwrap();
            assert_eq!(PrivateEventCodec::decode(stored).unwrap(), event);
        }
    }

    #[test]
    fn historical_opened_v0_fixture_is_upcast() {
        let payload: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/private_event_auction_opened_v0.json"
        ))
        .unwrap();
        let decoded = PrivateEventCodec::decode(StoredPrivateEvent {
            event_type: "AuctionOpened".to_owned(),
            schema_version: 0,
            payload,
        })
        .unwrap();
        assert!(matches!(
            decoded,
            AuctionEvent::Opened {
                bid_step_minor: 10,
                ..
            }
        ));
    }

    #[test]
    fn snapshot_round_trips_full_private_state() {
        let opened = Auction::default()
            .decide(AuctionCommand::Open {
                auction_id: AuctionId::from("auction-1"),
                lot_id: LotId::from("lot-1"),
                seller_id: SellerId::from("seller-1"),
                start_price: money(100),
                bid_step_minor: 10,
                closing_policy: policy(),
                opened_at: Timestamp::from_millis(100),
            })
            .unwrap();
        let mut auction = Auction::rehydrate(opened.events).unwrap();
        let bid = auction
            .decide(AuctionCommand::PlaceBid {
                bid_id: BidId::from("bid-1"),
                bidder_id: BidderId::from("user-1"),
                maximum: money(300),
                placed_at: Timestamp::from_millis(200),
                payment_instrument_ready: true,
            })
            .unwrap();
        for event in bid.events {
            auction.apply(event).unwrap();
        }

        let stored = SnapshotCodec::encode(&auction).unwrap();
        let decoded = SnapshotCodec::decode(stored).unwrap().unwrap();
        assert_eq!(decoded.snapshot(), auction.snapshot());
        assert!(matches!(
            decoded
                .decide(AuctionCommand::Cancel)
                .unwrap()
                .public_events[0],
            PublicEvent::AuctionCancelled { .. }
        ));
    }

    #[test]
    fn unknown_or_corrupt_snapshot_falls_back_to_stream_replay() {
        assert!(
            SnapshotCodec::decode(StoredSnapshot {
                schema_version: 99,
                payload: Value::Null,
            })
            .unwrap()
            .is_none()
        );
        assert!(
            SnapshotCodec::decode(StoredSnapshot {
                schema_version: 1,
                payload: serde_json::json!({"broken": true}),
            })
            .unwrap()
            .is_none()
        );
    }
}
