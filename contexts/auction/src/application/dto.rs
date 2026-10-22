use serde::{Deserialize, Serialize};

use crate::domain::{AuctionStatus, Money, Timestamp};

/// Identity established by a trusted inbound adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub kind: PrincipalKind,
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrincipalKind {
    User,
    ServiceAccount,
}

/// Metadata common to every changing use case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandContext {
    pub principal: Principal,
    pub idempotency_key: String,
    pub correlation_id: String,
    pub causation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceBidInput {
    pub auction_id: String,
    pub bid_id: String,
    pub maximum_minor: u64,
    pub currency_code: String,
    pub placed_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RescheduleAuctionInput {
    pub auction_id: String,
    pub scheduled_end: Timestamp,
    pub now: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseAuctionInput {
    pub auction_id: String,
    pub now: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelAuctionInput {
    pub auction_id: String,
    pub occurred_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForfeitSettlementCandidateInput {
    pub auction_id: String,
    pub candidate_id: String,
    pub settlement_round: u32,
    pub occurred_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmWinnerInput {
    pub auction_id: String,
    pub candidate_id: String,
    pub payment_id: String,
    pub occurred_at: Timestamp,
}

/// Public money representation. It can never contain a bidder maximum by type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoneyView {
    pub minor_units: u64,
    pub currency_code: String,
}

impl From<&Money> for MoneyView {
    fn from(value: &Money) -> Self {
        Self {
            minor_units: value.amount_minor(),
            currency_code: value.currency().to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentPriceView {
    pub price: MoneyView,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionView {
    pub auction_id: String,
    pub lot_id: String,
    pub status: PublicAuctionStatus,
    pub current_price: MoneyView,
    pub ends_at_millis: u64,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PublicAuctionStatus {
    Scheduled,
    Open,
    Settling,
    Sold,
    Unsold,
    Cancelled,
}

impl From<AuctionStatus> for PublicAuctionStatus {
    fn from(value: AuctionStatus) -> Self {
        match value {
            AuctionStatus::Uninitialized => Self::Scheduled,
            AuctionStatus::Open => Self::Open,
            AuctionStatus::Settling => Self::Settling,
            AuctionStatus::Sold => Self::Sold,
            AuctionStatus::Unsold => Self::Unsold,
            AuctionStatus::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceBidResult {
    pub auction: AuctionView,
    pub accepted_version: u64,
    pub outbid: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LifecycleResult {
    Closed {
        status: PublicAuctionStatus,
        closing_price: MoneyView,
        version: u64,
    },
    Cancelled {
        version: u64,
    },
    SettlementAdvanced {
        status: PublicAuctionStatus,
        settlement_round: u32,
        version: u64,
    },
    WinnerConfirmed {
        version: u64,
    },
}
