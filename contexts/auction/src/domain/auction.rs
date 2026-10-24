use std::{collections::BTreeMap, error::Error, fmt};

use super::{
    AuctionId, BidId, BidderAlias, BidderId, ClosingPolicy, ClosingPolicyError, LotId, Money,
    PaymentId, SellerId, Timestamp,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuctionCommand {
    Open {
        auction_id: AuctionId,
        lot_id: LotId,
        seller_id: SellerId,
        start_price: Money,
        bid_step_minor: u64,
        closing_policy: ClosingPolicy,
        opened_at: Timestamp,
    },
    PlaceBid {
        bid_id: BidId,
        bidder_id: BidderId,
        maximum: Money,
        placed_at: Timestamp,
        payment_instrument_ready: bool,
    },
    Reschedule {
        seller_id: SellerId,
        scheduled_end: Timestamp,
        now: Timestamp,
    },
    Close {
        now: Timestamp,
    },
    Cancel,
    ForfeitSettlementCandidate {
        bidder_id: BidderId,
        round: u32,
    },
    ConfirmWinner {
        bidder_id: BidderId,
        payment_id: PaymentId,
    },
}

struct OpenAuction {
    auction_id: AuctionId,
    lot_id: LotId,
    seller_id: SellerId,
    start_price: Money,
    bid_step_minor: u64,
    closing_policy: ClosingPolicy,
    opened_at: Timestamp,
}

/// Private events belonging to the authoritative auction stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuctionEvent {
    Opened {
        auction_id: AuctionId,
        lot_id: LotId,
        seller_id: SellerId,
        start_price: Money,
        bid_step_minor: u64,
        closing_policy: ClosingPolicy,
        opened_at: Timestamp,
        ends_at: Timestamp,
    },
    MaximumBidPlaced {
        bid_id: BidId,
        bidder_id: BidderId,
        bidder_alias: BidderAlias,
        maximum: Money,
        placed_at: Timestamp,
        first_bid_sequence: u64,
    },
    BidderOutbid {
        bidder_id: BidderId,
        bid_id: BidId,
    },
    DeadlineExtended {
        ends_at: Timestamp,
    },
    Rescheduled {
        scheduled_end: Timestamp,
        ends_at: Timestamp,
    },
    Closed {
        closing_price: Money,
    },
    Cancelled,
    SettlementCandidateSelected {
        bidder_id: BidderId,
        amount: Money,
        round: u32,
    },
    SettlementCandidateForfeited {
        bidder_id: BidderId,
        round: u32,
    },
    WinnerConfirmed {
        bidder_id: BidderId,
        payment_id: PaymentId,
    },
    Unsold,
}

/// Events safe to put in the public outbox. Secret maxima and bidder ids are
/// intentionally not representable here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicEvent {
    AuctionOpened {
        auction_id: AuctionId,
        ends_at: Timestamp,
        version: u64,
    },
    BidRecorded {
        bid_id: BidId,
        auction_id: AuctionId,
        bidder_alias: BidderAlias,
        resulting_price: Money,
        occurred_at: Timestamp,
        version: u64,
    },
    PriceChanged {
        auction_id: AuctionId,
        price: Money,
        ends_at: Timestamp,
        version: u64,
    },
    DeadlineExtended {
        auction_id: AuctionId,
        ends_at: Timestamp,
        version: u64,
    },
    AuctionRescheduled {
        auction_id: AuctionId,
        ends_at: Timestamp,
        version: u64,
    },
    AuctionClosed {
        auction_id: AuctionId,
        closing_price: Money,
        version: u64,
    },
    AuctionCancelled {
        auction_id: AuctionId,
        version: u64,
    },
    AuctionUnsold {
        auction_id: AuctionId,
        version: u64,
    },
    WinnerConfirmed {
        auction_id: AuctionId,
        version: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub events: Vec<AuctionEvent>,
    pub public_events: Vec<PublicEvent>,
    pub resulting_version: u64,
}

impl Decision {
    fn empty(version: u64) -> Self {
        Self {
            events: Vec::new(),
            public_events: Vec::new(),
            resulting_version: version,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuctionStatus {
    Uninitialized,
    Open,
    Settling,
    Sold,
    Unsold,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bid {
    maximum: Money,
    alias: BidderAlias,
    first_bid_sequence: u64,
    last_placed_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SettlementCandidate {
    bidder_id: BidderId,
    amount: Money,
    round: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BidSnapshot {
    pub bidder_id: BidderId,
    pub maximum: Money,
    pub alias: BidderAlias,
    pub first_bid_sequence: u64,
    pub last_placed_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SettlementCandidateSnapshot {
    pub bidder_id: BidderId,
    pub amount: Money,
    pub round: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuctionSnapshot {
    pub version: u64,
    pub status: AuctionStatus,
    pub id: Option<AuctionId>,
    pub lot_id: Option<LotId>,
    pub seller_id: Option<SellerId>,
    pub start_price: Option<Money>,
    pub bid_step_minor: u64,
    pub closing_policy: Option<ClosingPolicy>,
    pub opened_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
    pub bids: Vec<BidSnapshot>,
    pub next_bid_sequence: u64,
    pub current_price: Option<Money>,
    pub closing_price: Option<Money>,
    pub forfeited: Vec<BidderId>,
    pub settlement_candidate: Option<SettlementCandidateSnapshot>,
    pub winner: Option<BidderId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Auction {
    version: u64,
    status: AuctionStatus,
    id: Option<AuctionId>,
    lot_id: Option<LotId>,
    seller_id: Option<SellerId>,
    start_price: Option<Money>,
    bid_step_minor: u64,
    closing_policy: Option<ClosingPolicy>,
    opened_at: Option<Timestamp>,
    ends_at: Option<Timestamp>,
    bids: BTreeMap<BidderId, Bid>,
    next_bid_sequence: u64,
    current_price: Option<Money>,
    closing_price: Option<Money>,
    forfeited: Vec<BidderId>,
    settlement_candidate: Option<SettlementCandidate>,
    winner: Option<BidderId>,
}

impl Default for Auction {
    fn default() -> Self {
        Self {
            version: 0,
            status: AuctionStatus::Uninitialized,
            id: None,
            lot_id: None,
            seller_id: None,
            start_price: None,
            bid_step_minor: 0,
            closing_policy: None,
            opened_at: None,
            ends_at: None,
            bids: BTreeMap::new(),
            next_bid_sequence: 1,
            current_price: None,
            closing_price: None,
            forfeited: Vec::new(),
            settlement_candidate: None,
            winner: None,
        }
    }
}

impl Auction {
    pub fn rehydrate(events: impl IntoIterator<Item = AuctionEvent>) -> Result<Self, AuctionError> {
        let mut auction = Self::default();
        for event in events {
            auction.apply(event)?;
        }
        Ok(auction)
    }

    pub(crate) fn snapshot(&self) -> AuctionSnapshot {
        AuctionSnapshot {
            version: self.version,
            status: self.status,
            id: self.id.clone(),
            lot_id: self.lot_id.clone(),
            seller_id: self.seller_id.clone(),
            start_price: self.start_price.clone(),
            bid_step_minor: self.bid_step_minor,
            closing_policy: self.closing_policy.clone(),
            opened_at: self.opened_at,
            ends_at: self.ends_at,
            bids: self
                .bids
                .iter()
                .map(|(bidder_id, bid)| BidSnapshot {
                    bidder_id: bidder_id.clone(),
                    maximum: bid.maximum.clone(),
                    alias: bid.alias.clone(),
                    first_bid_sequence: bid.first_bid_sequence,
                    last_placed_at: bid.last_placed_at,
                })
                .collect(),
            next_bid_sequence: self.next_bid_sequence,
            current_price: self.current_price.clone(),
            closing_price: self.closing_price.clone(),
            forfeited: self.forfeited.clone(),
            settlement_candidate: self.settlement_candidate.as_ref().map(|candidate| {
                SettlementCandidateSnapshot {
                    bidder_id: candidate.bidder_id.clone(),
                    amount: candidate.amount.clone(),
                    round: candidate.round,
                }
            }),
            winner: self.winner.clone(),
        }
    }

    pub(crate) fn from_snapshot(snapshot: AuctionSnapshot) -> Result<Self, AuctionError> {
        if snapshot.version == 0 {
            return (snapshot.status == AuctionStatus::Uninitialized)
                .then(Self::default)
                .ok_or(AuctionError::CorruptEventStream(
                    "initialized snapshot has version zero",
                ));
        }
        if snapshot.id.is_none()
            || snapshot.lot_id.is_none()
            || snapshot.seller_id.is_none()
            || snapshot.start_price.is_none()
            || snapshot.closing_policy.is_none()
            || snapshot.opened_at.is_none()
            || snapshot.ends_at.is_none()
            || snapshot.current_price.is_none()
        {
            return Err(AuctionError::CorruptEventStream(
                "initialized snapshot misses required state",
            ));
        }
        let mut bids = BTreeMap::new();
        for bid in snapshot.bids {
            if bids
                .insert(
                    bid.bidder_id,
                    Bid {
                        maximum: bid.maximum,
                        alias: bid.alias,
                        first_bid_sequence: bid.first_bid_sequence,
                        last_placed_at: bid.last_placed_at,
                    },
                )
                .is_some()
            {
                return Err(AuctionError::CorruptEventStream(
                    "snapshot contains duplicate bidder",
                ));
            }
        }
        Ok(Self {
            version: snapshot.version,
            status: snapshot.status,
            id: snapshot.id,
            lot_id: snapshot.lot_id,
            seller_id: snapshot.seller_id,
            start_price: snapshot.start_price,
            bid_step_minor: snapshot.bid_step_minor,
            closing_policy: snapshot.closing_policy,
            opened_at: snapshot.opened_at,
            ends_at: snapshot.ends_at,
            bids,
            next_bid_sequence: snapshot.next_bid_sequence,
            current_price: snapshot.current_price,
            closing_price: snapshot.closing_price,
            forfeited: snapshot.forfeited,
            settlement_candidate: snapshot.settlement_candidate.map(|candidate| {
                SettlementCandidate {
                    bidder_id: candidate.bidder_id,
                    amount: candidate.amount,
                    round: candidate.round,
                }
            }),
            winner: snapshot.winner,
        })
    }

    pub const fn version(&self) -> u64 {
        self.version
    }

    pub const fn status(&self) -> AuctionStatus {
        self.status
    }

    pub fn id(&self) -> Option<&AuctionId> {
        self.id.as_ref()
    }

    pub fn lot_id(&self) -> Option<&LotId> {
        self.lot_id.as_ref()
    }

    pub fn seller_id(&self) -> Option<&SellerId> {
        self.seller_id.as_ref()
    }

    pub fn current_price(&self) -> Option<&Money> {
        self.current_price.as_ref()
    }

    pub fn closing_price(&self) -> Option<&Money> {
        self.closing_price.as_ref()
    }

    pub const fn ends_at(&self) -> Option<Timestamp> {
        self.ends_at
    }

    pub fn leader(&self) -> Option<&BidderId> {
        self.pricing(self.active_bids()).map(|(bidder, _)| bidder)
    }

    pub fn settlement_candidate(&self) -> Option<(&BidderId, &Money, u32)> {
        self.settlement_candidate
            .as_ref()
            .map(|candidate| (&candidate.bidder_id, &candidate.amount, candidate.round))
    }

    pub fn winner(&self) -> Option<&BidderId> {
        self.winner.as_ref()
    }

    pub fn decide(&self, command: AuctionCommand) -> Result<Decision, AuctionError> {
        match command {
            AuctionCommand::Open {
                auction_id,
                lot_id,
                seller_id,
                start_price,
                bid_step_minor,
                closing_policy,
                opened_at,
            } => self.open(OpenAuction {
                auction_id,
                lot_id,
                seller_id,
                start_price,
                bid_step_minor,
                closing_policy,
                opened_at,
            }),
            AuctionCommand::PlaceBid {
                bid_id,
                bidder_id,
                maximum,
                placed_at,
                payment_instrument_ready,
            } => self.place_bid(
                bid_id,
                bidder_id,
                maximum,
                placed_at,
                payment_instrument_ready,
            ),
            AuctionCommand::Reschedule {
                seller_id,
                scheduled_end,
                now,
            } => self.reschedule(seller_id, scheduled_end, now),
            AuctionCommand::Close { now } => self.close(now),
            AuctionCommand::Cancel => self.cancel(),
            AuctionCommand::ForfeitSettlementCandidate { bidder_id, round } => {
                self.forfeit(bidder_id, round)
            }
            AuctionCommand::ConfirmWinner {
                bidder_id,
                payment_id,
            } => self.confirm_winner(bidder_id, payment_id),
        }
    }

    /// Fold one persisted event into the aggregate state.
    pub fn apply(&mut self, event: AuctionEvent) -> Result<(), AuctionError> {
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
            } => {
                if self.status != AuctionStatus::Uninitialized {
                    return Err(AuctionError::CorruptEventStream("auction opened twice"));
                }
                self.id = Some(auction_id);
                self.lot_id = Some(lot_id);
                self.seller_id = Some(seller_id);
                self.current_price = Some(start_price.clone());
                self.start_price = Some(start_price);
                self.bid_step_minor = bid_step_minor;
                self.closing_policy = Some(closing_policy);
                self.opened_at = Some(opened_at);
                self.ends_at = Some(ends_at);
                self.status = AuctionStatus::Open;
            }
            AuctionEvent::MaximumBidPlaced {
                bidder_id,
                bidder_alias,
                maximum,
                placed_at,
                first_bid_sequence,
                ..
            } => {
                self.next_bid_sequence = self.next_bid_sequence.max(first_bid_sequence + 1);
                self.bids.insert(
                    bidder_id,
                    Bid {
                        maximum,
                        alias: bidder_alias,
                        first_bid_sequence,
                        last_placed_at: placed_at,
                    },
                );
                self.current_price = Some(self.calculate_price(self.active_bids())?.1);
            }
            AuctionEvent::BidderOutbid { .. } => {}
            AuctionEvent::DeadlineExtended { ends_at } => self.ends_at = Some(ends_at),
            AuctionEvent::Rescheduled {
                scheduled_end,
                ends_at,
            } => {
                let policy = self
                    .closing_policy
                    .as_ref()
                    .ok_or(AuctionError::CorruptEventStream("missing closing policy"))?;
                self.closing_policy = Some(policy.rescheduled(scheduled_end)?);
                self.ends_at = Some(ends_at);
            }
            AuctionEvent::Closed { closing_price } => {
                self.status = AuctionStatus::Settling;
                self.closing_price = Some(closing_price);
            }
            AuctionEvent::Cancelled => self.status = AuctionStatus::Cancelled,
            AuctionEvent::SettlementCandidateSelected {
                bidder_id,
                amount,
                round,
            } => {
                self.status = AuctionStatus::Settling;
                self.settlement_candidate = Some(SettlementCandidate {
                    bidder_id,
                    amount,
                    round,
                });
            }
            AuctionEvent::SettlementCandidateForfeited { bidder_id, .. } => {
                self.forfeited.push(bidder_id);
                self.settlement_candidate = None;
            }
            AuctionEvent::WinnerConfirmed { bidder_id, .. } => {
                self.winner = Some(bidder_id);
                self.settlement_candidate = None;
                self.status = AuctionStatus::Sold;
            }
            AuctionEvent::Unsold => {
                self.settlement_candidate = None;
                self.status = AuctionStatus::Unsold;
            }
        }
        self.version = self.version.checked_add(1).ok_or(AuctionError::Overflow)?;
        Ok(())
    }

    fn open(&self, command: OpenAuction) -> Result<Decision, AuctionError> {
        let OpenAuction {
            auction_id,
            lot_id,
            seller_id,
            start_price,
            bid_step_minor,
            closing_policy,
            opened_at,
        } = command;
        if self.status != AuctionStatus::Uninitialized {
            return Err(AuctionError::AlreadyOpened);
        }
        if bid_step_minor == 0 {
            return Err(AuctionError::InvalidBidStep);
        }
        let ends_at = closing_policy.initial_end(opened_at)?;
        if ends_at <= opened_at {
            return Err(AuctionError::DeadlineNotFuture);
        }
        let events = vec![AuctionEvent::Opened {
            auction_id: auction_id.clone(),
            lot_id,
            seller_id,
            start_price,
            bid_step_minor,
            closing_policy,
            opened_at,
            ends_at,
        }];
        let public_events = vec![PublicEvent::AuctionOpened {
            auction_id,
            ends_at,
            version: 1,
        }];
        Ok(self.decision(events, public_events))
    }

    fn place_bid(
        &self,
        bid_id: BidId,
        bidder_id: BidderId,
        maximum: Money,
        placed_at: Timestamp,
        payment_instrument_ready: bool,
    ) -> Result<Decision, AuctionError> {
        self.require_open()?;
        if !payment_instrument_ready {
            return Err(AuctionError::PaymentInstrumentRequired);
        }
        let ends_at = self.required_ends_at()?;
        if placed_at >= ends_at {
            return Err(AuctionError::BiddingEnded);
        }
        let current_price = self.required_current_price()?;
        if !maximum.same_currency(current_price) {
            return Err(AuctionError::CurrencyMismatch);
        }
        if maximum.amount_minor() <= current_price.amount_minor() {
            return Err(AuctionError::MaximumNotAboveCurrentPrice {
                current_price: current_price.clone(),
            });
        }
        if let Some(previous) = self.bids.get(&bidder_id)
            && maximum.amount_minor() <= previous.maximum.amount_minor()
        {
            return Err(AuctionError::MaximumNotRaised);
        }

        let previous_leader = self.leader().cloned();
        let (alias, first_bid_sequence) = self.bids.get(&bidder_id).map_or_else(
            || {
                (
                    BidderAlias::from_sequence(self.next_bid_sequence),
                    self.next_bid_sequence,
                )
            },
            |bid| (bid.alias.clone(), bid.first_bid_sequence),
        );
        let mut prospective_bids = self.bids.clone();
        prospective_bids.insert(
            bidder_id.clone(),
            Bid {
                maximum: maximum.clone(),
                alias: alias.clone(),
                first_bid_sequence,
                last_placed_at: placed_at,
            },
        );
        let (next_leader, resulting_price) = self.calculate_price(prospective_bids.iter())?;
        let next_leader = next_leader.clone();
        let new_ends_at = self
            .closing_policy
            .as_ref()
            .ok_or(AuctionError::CorruptEventStream("missing closing policy"))?
            .end_after_bid(placed_at)?;

        let mut events = vec![AuctionEvent::MaximumBidPlaced {
            bid_id: bid_id.clone(),
            bidder_id: bidder_id.clone(),
            bidder_alias: alias.clone(),
            maximum,
            placed_at,
            first_bid_sequence,
        }];
        let outbid_bidder = previous_leader
            .filter(|previous| previous != &next_leader)
            .or_else(|| (next_leader != bidder_id).then_some(bidder_id));
        if let Some(bidder_id) = outbid_bidder {
            events.push(AuctionEvent::BidderOutbid {
                bidder_id,
                bid_id: bid_id.clone(),
            });
        }
        if new_ends_at > ends_at {
            events.push(AuctionEvent::DeadlineExtended {
                ends_at: new_ends_at,
            });
        }
        let version = self
            .version
            .checked_add(events.len() as u64)
            .ok_or(AuctionError::Overflow)?;
        let auction_id = self.required_id()?.clone();
        let mut public_events = vec![PublicEvent::BidRecorded {
            bid_id,
            auction_id: auction_id.clone(),
            bidder_alias: alias,
            resulting_price: resulting_price.clone(),
            occurred_at: placed_at,
            version,
        }];
        if &resulting_price != current_price {
            public_events.push(PublicEvent::PriceChanged {
                auction_id: auction_id.clone(),
                price: resulting_price,
                ends_at: new_ends_at.max(ends_at),
                version,
            });
        }
        if new_ends_at > ends_at {
            public_events.push(PublicEvent::DeadlineExtended {
                auction_id,
                ends_at: new_ends_at,
                version,
            });
        }
        Ok(self.decision(events, public_events))
    }

    fn reschedule(
        &self,
        seller_id: SellerId,
        scheduled_end: Timestamp,
        now: Timestamp,
    ) -> Result<Decision, AuctionError> {
        self.require_open()?;
        if self.seller_id.as_ref() != Some(&seller_id) {
            return Err(AuctionError::NotSeller);
        }
        if scheduled_end <= now {
            return Err(AuctionError::DeadlineNotFuture);
        }
        let policy = self
            .closing_policy
            .as_ref()
            .ok_or(AuctionError::CorruptEventStream("missing closing policy"))?
            .rescheduled(scheduled_end)?;
        let reference = self
            .bids
            .values()
            .map(|bid| bid.last_placed_at)
            .max()
            .or(self.opened_at)
            .ok_or(AuctionError::CorruptEventStream("missing opening time"))?;
        let ends_at = policy.end_after_bid(reference)?;
        let events = vec![AuctionEvent::Rescheduled {
            scheduled_end,
            ends_at,
        }];
        let version = self.version.checked_add(1).ok_or(AuctionError::Overflow)?;
        let public_events = vec![PublicEvent::AuctionRescheduled {
            auction_id: self.required_id()?.clone(),
            ends_at,
            version,
        }];
        Ok(self.decision(events, public_events))
    }

    fn close(&self, now: Timestamp) -> Result<Decision, AuctionError> {
        if matches!(
            self.status,
            AuctionStatus::Settling | AuctionStatus::Sold | AuctionStatus::Unsold
        ) {
            return Ok(Decision::empty(self.version));
        }
        self.require_open()?;
        let ends_at = self.required_ends_at()?;
        if now < ends_at {
            return Err(AuctionError::DeadlineNotReached { ends_at });
        }
        let closing_price = self.required_current_price()?.clone();
        let mut events = vec![AuctionEvent::Closed {
            closing_price: closing_price.clone(),
        }];
        match self.calculate_price(self.active_bids()) {
            Ok((bidder_id, amount)) => events.push(AuctionEvent::SettlementCandidateSelected {
                bidder_id: bidder_id.clone(),
                amount,
                round: 1,
            }),
            Err(AuctionError::NoEligibleBids) => events.push(AuctionEvent::Unsold),
            Err(error) => return Err(error),
        }
        let version = self
            .version
            .checked_add(events.len() as u64)
            .ok_or(AuctionError::Overflow)?;
        let auction_id = self.required_id()?.clone();
        let mut public_events = vec![PublicEvent::AuctionClosed {
            auction_id: auction_id.clone(),
            closing_price,
            version,
        }];
        if matches!(events.last(), Some(AuctionEvent::Unsold)) {
            public_events.push(PublicEvent::AuctionUnsold {
                auction_id,
                version,
            });
        }
        Ok(self.decision(events, public_events))
    }

    fn cancel(&self) -> Result<Decision, AuctionError> {
        if self.status == AuctionStatus::Cancelled {
            return Ok(Decision::empty(self.version));
        }
        self.require_open()?;
        let events = vec![AuctionEvent::Cancelled];
        let version = self.version.checked_add(1).ok_or(AuctionError::Overflow)?;
        let public_events = vec![PublicEvent::AuctionCancelled {
            auction_id: self.required_id()?.clone(),
            version,
        }];
        Ok(self.decision(events, public_events))
    }

    fn forfeit(&self, bidder_id: BidderId, round: u32) -> Result<Decision, AuctionError> {
        if self.status != AuctionStatus::Settling {
            return Err(AuctionError::NotSettling);
        }
        let candidate = self
            .settlement_candidate
            .as_ref()
            .ok_or(AuctionError::NoSettlementCandidate)?;
        if candidate.bidder_id != bidder_id || candidate.round != round {
            return Err(AuctionError::SettlementCandidateMismatch);
        }

        let mut events = vec![AuctionEvent::SettlementCandidateForfeited {
            bidder_id: bidder_id.clone(),
            round,
        }];
        let remaining = self
            .bids
            .iter()
            .filter(|(id, _)| *id != &bidder_id && !self.forfeited.contains(*id));
        match self.calculate_price(remaining) {
            Ok((next_bidder, amount)) => {
                events.push(AuctionEvent::SettlementCandidateSelected {
                    bidder_id: next_bidder.clone(),
                    amount,
                    round: round.checked_add(1).ok_or(AuctionError::Overflow)?,
                });
            }
            Err(AuctionError::NoEligibleBids) => events.push(AuctionEvent::Unsold),
            Err(error) => return Err(error),
        }
        let version = self
            .version
            .checked_add(events.len() as u64)
            .ok_or(AuctionError::Overflow)?;
        let public_events = if matches!(events.last(), Some(AuctionEvent::Unsold)) {
            vec![PublicEvent::AuctionUnsold {
                auction_id: self.required_id()?.clone(),
                version,
            }]
        } else {
            Vec::new()
        };
        Ok(self.decision(events, public_events))
    }

    fn confirm_winner(
        &self,
        bidder_id: BidderId,
        payment_id: PaymentId,
    ) -> Result<Decision, AuctionError> {
        if self.status == AuctionStatus::Sold && self.winner.as_ref() == Some(&bidder_id) {
            return Ok(Decision::empty(self.version));
        }
        if self.status != AuctionStatus::Settling {
            return Err(AuctionError::NotSettling);
        }
        let candidate = self
            .settlement_candidate
            .as_ref()
            .ok_or(AuctionError::NoSettlementCandidate)?;
        if candidate.bidder_id != bidder_id {
            return Err(AuctionError::SettlementCandidateMismatch);
        }
        let events = vec![AuctionEvent::WinnerConfirmed {
            bidder_id,
            payment_id,
        }];
        let version = self.version.checked_add(1).ok_or(AuctionError::Overflow)?;
        let public_events = vec![PublicEvent::WinnerConfirmed {
            auction_id: self.required_id()?.clone(),
            version,
        }];
        Ok(self.decision(events, public_events))
    }

    fn decision(&self, events: Vec<AuctionEvent>, public_events: Vec<PublicEvent>) -> Decision {
        Decision {
            resulting_version: self.version + events.len() as u64,
            events,
            public_events,
        }
    }

    fn require_open(&self) -> Result<(), AuctionError> {
        match self.status {
            AuctionStatus::Uninitialized => Err(AuctionError::NotOpened),
            AuctionStatus::Open => Ok(()),
            _ => Err(AuctionError::NotOpen),
        }
    }

    fn required_id(&self) -> Result<&AuctionId, AuctionError> {
        self.id
            .as_ref()
            .ok_or(AuctionError::CorruptEventStream("missing auction id"))
    }

    fn required_current_price(&self) -> Result<&Money, AuctionError> {
        self.current_price
            .as_ref()
            .ok_or(AuctionError::CorruptEventStream("missing current price"))
    }

    fn required_ends_at(&self) -> Result<Timestamp, AuctionError> {
        self.ends_at
            .ok_or(AuctionError::CorruptEventStream("missing deadline"))
    }

    fn active_bids(&self) -> impl Iterator<Item = (&BidderId, &Bid)> + Clone {
        self.bids
            .iter()
            .filter(|(bidder_id, _)| !self.forfeited.contains(*bidder_id))
    }

    fn pricing<'a>(
        &self,
        bids: impl Iterator<Item = (&'a BidderId, &'a Bid)>,
    ) -> Option<(&'a BidderId, &'a Bid)> {
        bids.max_by(|(_, left), (_, right)| {
            left.maximum
                .amount_minor()
                .cmp(&right.maximum.amount_minor())
                .then_with(|| right.first_bid_sequence.cmp(&left.first_bid_sequence))
        })
    }

    fn calculate_price<'a>(
        &self,
        bids: impl Iterator<Item = (&'a BidderId, &'a Bid)> + Clone,
    ) -> Result<(&'a BidderId, Money), AuctionError> {
        let (leader_id, leader) = self
            .pricing(bids.clone())
            .ok_or(AuctionError::NoEligibleBids)?;
        let second_maximum = bids
            .filter(|(bidder_id, _)| *bidder_id != leader_id)
            .map(|(_, bid)| bid.maximum.amount_minor())
            .max();
        let start_price = self
            .start_price
            .as_ref()
            .ok_or(AuctionError::CorruptEventStream("missing start price"))?;
        let amount = match second_maximum {
            None => start_price.amount_minor(),
            Some(second) => leader.maximum.amount_minor().min(
                second
                    .checked_add(self.bid_step_minor)
                    .ok_or(AuctionError::Overflow)?,
            ),
        };
        Ok((leader_id, start_price.with_amount(amount)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuctionError {
    AlreadyOpened,
    NotOpened,
    NotOpen,
    NotSettling,
    InvalidBidStep,
    DeadlineNotFuture,
    DeadlineNotReached { ends_at: Timestamp },
    BiddingEnded,
    NotSeller,
    PaymentInstrumentRequired,
    CurrencyMismatch,
    MaximumNotAboveCurrentPrice { current_price: Money },
    MaximumNotRaised,
    NoEligibleBids,
    NoSettlementCandidate,
    SettlementCandidateMismatch,
    Overflow,
    ClosingPolicy(ClosingPolicyError),
    CorruptEventStream(&'static str),
}

impl fmt::Display for AuctionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyOpened => write!(formatter, "auction is already open"),
            Self::NotOpened => write!(formatter, "auction has not been opened"),
            Self::NotOpen => write!(formatter, "auction is not open"),
            Self::NotSettling => write!(formatter, "auction is not settling"),
            Self::InvalidBidStep => write!(formatter, "bid step must be positive"),
            Self::DeadlineNotFuture => write!(formatter, "deadline must be in the future"),
            Self::DeadlineNotReached { .. } => {
                write!(formatter, "auction deadline has not been reached")
            }
            Self::BiddingEnded => write!(formatter, "bidding deadline has passed"),
            Self::NotSeller => write!(formatter, "only the seller may reschedule this auction"),
            Self::PaymentInstrumentRequired => {
                write!(formatter, "a ready payment instrument is required")
            }
            Self::CurrencyMismatch => {
                write!(formatter, "bid currency differs from auction currency")
            }
            Self::MaximumNotAboveCurrentPrice { .. } => {
                write!(formatter, "maximum must exceed the current price")
            }
            Self::MaximumNotRaised => {
                write!(formatter, "a bidder may only raise their own maximum")
            }
            Self::NoEligibleBids => write!(formatter, "there are no eligible bids"),
            Self::NoSettlementCandidate => write!(formatter, "there is no settlement candidate"),
            Self::SettlementCandidateMismatch => {
                write!(formatter, "settlement candidate or round does not match")
            }
            Self::Overflow => write!(formatter, "domain arithmetic overflow"),
            Self::ClosingPolicy(error) => error.fmt(formatter),
            Self::CorruptEventStream(reason) => {
                write!(formatter, "corrupt auction event stream: {reason}")
            }
        }
    }
}

impl Error for AuctionError {}

impl From<ClosingPolicyError> for AuctionError {
    fn from(value: ClosingPolicyError) -> Self {
        Self::ClosingPolicy(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DurationMillis;

    fn ts(value: u64) -> Timestamp {
        Timestamp::from_millis(value)
    }

    fn money(value: u64) -> Money {
        Money::new(value, "USD").unwrap()
    }

    fn open_auction(policy: ClosingPolicy) -> Auction {
        execute(
            Auction::default(),
            AuctionCommand::Open {
                auction_id: "auction-1".into(),
                lot_id: "lot-1".into(),
                seller_id: "seller-1".into(),
                start_price: money(100),
                bid_step_minor: 10,
                closing_policy: policy,
                opened_at: ts(1_000),
            },
        )
        .unwrap()
        .0
    }

    fn scheduled_auction() -> Auction {
        open_auction(ClosingPolicy::new(Some(ts(10_000)), None, None).unwrap())
    }

    fn bid(auction: Auction, id: &str, bidder: &str, maximum: u64, at: u64) -> Auction {
        execute(
            auction,
            AuctionCommand::PlaceBid {
                bid_id: id.into(),
                bidder_id: bidder.into(),
                maximum: money(maximum),
                placed_at: ts(at),
                payment_instrument_ready: true,
            },
        )
        .unwrap()
        .0
    }

    fn execute(
        mut auction: Auction,
        command: AuctionCommand,
    ) -> Result<(Auction, Decision), AuctionError> {
        let decision = auction.decide(command)?;
        for event in decision.events.iter().cloned() {
            auction.apply(event)?;
        }
        assert_eq!(auction.version(), decision.resulting_version);
        Ok((auction, decision))
    }

    #[test]
    fn first_bid_keeps_start_price_and_never_publishes_maximum() {
        let (auction, decision) = execute(
            scheduled_auction(),
            AuctionCommand::PlaceBid {
                bid_id: "bid-1".into(),
                bidder_id: "alice".into(),
                maximum: money(200),
                placed_at: ts(2_000),
                payment_instrument_ready: true,
            },
        )
        .unwrap();

        assert_eq!(auction.current_price(), Some(&money(100)));
        assert_eq!(auction.leader(), Some(&BidderId::from("alice")));
        assert_eq!(decision.public_events.len(), 1);
        assert!(matches!(
            &decision.public_events[0],
            PublicEvent::BidRecorded { resulting_price, .. } if resulting_price == &money(100)
        ));
    }

    #[test]
    fn proxy_bidding_uses_second_maximum_plus_step() {
        let auction = bid(scheduled_auction(), "bid-1", "alice", 200, 2_000);
        let auction = bid(auction, "bid-2", "bob", 150, 3_000);

        assert_eq!(auction.leader(), Some(&BidderId::from("alice")));
        assert_eq!(auction.current_price(), Some(&money(160)));
    }

    #[test]
    fn outbid_fact_targets_previous_leader_when_lead_changes() {
        let auction = bid(scheduled_auction(), "bid-1", "alice", 200, 2_000);
        let (_, decision) = execute(
            auction,
            AuctionCommand::PlaceBid {
                bid_id: "bid-2".into(),
                bidder_id: "bob".into(),
                maximum: money(300),
                placed_at: ts(3_000),
                payment_instrument_ready: true,
            },
        )
        .unwrap();

        assert!(matches!(
            decision.events.as_slice(),
            [
                AuctionEvent::MaximumBidPlaced { .. },
                AuctionEvent::BidderOutbid { bidder_id, bid_id }
            ] if bidder_id == &BidderId::from("alice") && bid_id == &BidId::from("bid-2")
        ));
    }

    #[test]
    fn outbid_fact_targets_challenger_when_proxy_maximum_holds() {
        let auction = bid(scheduled_auction(), "bid-1", "alice", 300, 2_000);
        let (_, decision) = execute(
            auction,
            AuctionCommand::PlaceBid {
                bid_id: "bid-2".into(),
                bidder_id: "bob".into(),
                maximum: money(200),
                placed_at: ts(3_000),
                payment_instrument_ready: true,
            },
        )
        .unwrap();

        assert!(matches!(
            decision.events.as_slice(),
            [
                AuctionEvent::MaximumBidPlaced { .. },
                AuctionEvent::BidderOutbid { bidder_id, bid_id }
            ] if bidder_id == &BidderId::from("bob") && bid_id == &BidId::from("bid-2")
        ));
    }

    #[test]
    fn leader_raising_own_maximum_creates_no_outbid_fact() {
        let auction = bid(scheduled_auction(), "bid-1", "alice", 200, 2_000);
        let (_, decision) = execute(
            auction,
            AuctionCommand::PlaceBid {
                bid_id: "bid-2".into(),
                bidder_id: "alice".into(),
                maximum: money(300),
                placed_at: ts(3_000),
                payment_instrument_ready: true,
            },
        )
        .unwrap();

        assert_eq!(decision.events.len(), 1);
        assert!(matches!(
            decision.events[0],
            AuctionEvent::MaximumBidPlaced { .. }
        ));
    }

    #[test]
    fn bidder_outbid_replay_is_a_no_op_except_for_stream_version() {
        let mut auction = bid(scheduled_auction(), "bid-1", "alice", 200, 2_000);
        let before_price = auction.current_price().cloned();
        let before_leader = auction.leader().cloned();
        let before_bids = auction.bids.clone();
        let before_version = auction.version();

        auction
            .apply(AuctionEvent::BidderOutbid {
                bidder_id: BidderId::from("bob"),
                bid_id: BidId::from("bid-2"),
            })
            .unwrap();

        assert_eq!(auction.version(), before_version + 1);
        assert_eq!(auction.current_price(), before_price.as_ref());
        assert_eq!(auction.leader(), before_leader.as_ref());
        assert_eq!(auction.bids, before_bids);
    }

    #[test]
    fn earlier_bid_wins_equal_maxima() {
        let auction = bid(scheduled_auction(), "bid-1", "alice", 200, 2_000);
        let auction = bid(auction, "bid-2", "bob", 200, 3_000);

        assert_eq!(auction.leader(), Some(&BidderId::from("alice")));
        assert_eq!(auction.current_price(), Some(&money(200)));
    }

    #[test]
    fn rejects_invalid_bids() {
        let auction = scheduled_auction();
        let no_payment = auction
            .decide(AuctionCommand::PlaceBid {
                bid_id: "bid-1".into(),
                bidder_id: "alice".into(),
                maximum: money(200),
                placed_at: ts(2_000),
                payment_instrument_ready: false,
            })
            .unwrap_err();
        assert_eq!(no_payment, AuctionError::PaymentInstrumentRequired);

        let auction = bid(auction, "bid-1", "alice", 200, 2_000);
        let not_raised = auction
            .decide(AuctionCommand::PlaceBid {
                bid_id: "bid-2".into(),
                bidder_id: "alice".into(),
                maximum: money(190),
                placed_at: ts(3_000),
                payment_instrument_ready: true,
            })
            .unwrap_err();
        assert_eq!(not_raised, AuctionError::MaximumNotRaised);
    }

    #[test]
    fn inactivity_extends_deadline_but_never_past_hard_deadline() {
        let policy =
            ClosingPolicy::new(Some(ts(5_000)), DurationMillis::new(4_000), Some(ts(8_000)))
                .unwrap();
        let auction = open_auction(policy);
        let (auction, decision) = execute(
            auction,
            AuctionCommand::PlaceBid {
                bid_id: "bid-1".into(),
                bidder_id: "alice".into(),
                maximum: money(200),
                placed_at: ts(4_500),
                payment_instrument_ready: true,
            },
        )
        .unwrap();

        assert_eq!(auction.ends_at(), Some(ts(8_000)));
        assert!(
            decision
                .events
                .contains(&AuctionEvent::DeadlineExtended { ends_at: ts(8_000) })
        );
    }

    #[test]
    fn seller_can_shorten_the_deadline() {
        let auction = scheduled_auction();
        let (auction, _) = execute(
            auction,
            AuctionCommand::Reschedule {
                seller_id: "seller-1".into(),
                scheduled_end: ts(8_000),
                now: ts(3_000),
            },
        )
        .unwrap();
        assert_eq!(auction.ends_at(), Some(ts(8_000)));
    }

    #[test]
    fn reschedule_preserves_inactivity_window_from_last_bid() {
        let policy = ClosingPolicy::new(
            Some(ts(10_000)),
            DurationMillis::new(4_000),
            Some(ts(20_000)),
        )
        .unwrap();
        let auction = bid(open_auction(policy), "bid-1", "alice", 200, 7_000);
        let (auction, _) = execute(
            auction,
            AuctionCommand::Reschedule {
                seller_id: "seller-1".into(),
                scheduled_end: ts(8_000),
                now: ts(7_500),
            },
        )
        .unwrap();

        assert_eq!(auction.ends_at(), Some(ts(11_000)));
    }

    #[test]
    fn close_is_too_early_then_idempotent_after_success() {
        let auction = bid(scheduled_auction(), "bid-1", "alice", 200, 2_000);
        assert_eq!(
            auction
                .decide(AuctionCommand::Close { now: ts(9_999) })
                .unwrap_err(),
            AuctionError::DeadlineNotReached {
                ends_at: ts(10_000)
            }
        );

        let (auction, _) = execute(auction, AuctionCommand::Close { now: ts(10_000) }).unwrap();
        assert_eq!(auction.status(), AuctionStatus::Settling);
        assert_eq!(
            auction.settlement_candidate(),
            Some((&BidderId::from("alice"), &money(100), 1))
        );
        assert!(
            auction
                .decide(AuctionCommand::Close { now: ts(11_000) })
                .unwrap()
                .events
                .is_empty()
        );
    }

    #[test]
    fn closes_without_bids_as_unsold() {
        let (auction, _) = execute(
            scheduled_auction(),
            AuctionCommand::Close { now: ts(10_000) },
        )
        .unwrap();
        assert_eq!(auction.status(), AuctionStatus::Unsold);
    }

    #[test]
    fn forfeiture_reprices_without_previous_candidate_and_keeps_closing_price() {
        let auction = bid(scheduled_auction(), "bid-1", "alice", 200, 2_000);
        let auction = bid(auction, "bid-2", "bob", 150, 3_000);
        let auction = bid(auction, "bid-3", "carol", 180, 4_000);
        let (auction, _) = execute(auction, AuctionCommand::Close { now: ts(10_000) }).unwrap();
        assert_eq!(auction.current_price(), Some(&money(190)));

        let (auction, _) = execute(
            auction,
            AuctionCommand::ForfeitSettlementCandidate {
                bidder_id: "alice".into(),
                round: 1,
            },
        )
        .unwrap();
        assert_eq!(
            auction.settlement_candidate(),
            Some((&BidderId::from("carol"), &money(160), 2))
        );
        assert_eq!(auction.current_price(), Some(&money(190)));
    }

    #[test]
    fn only_current_candidate_can_be_confirmed() {
        let auction = bid(scheduled_auction(), "bid-1", "alice", 200, 2_000);
        let (auction, _) = execute(auction, AuctionCommand::Close { now: ts(10_000) }).unwrap();
        assert_eq!(
            auction
                .decide(AuctionCommand::ConfirmWinner {
                    bidder_id: "bob".into(),
                    payment_id: "payment-1".into(),
                })
                .unwrap_err(),
            AuctionError::SettlementCandidateMismatch
        );

        let (auction, _) = execute(
            auction,
            AuctionCommand::ConfirmWinner {
                bidder_id: "alice".into(),
                payment_id: "payment-1".into(),
            },
        )
        .unwrap();
        assert_eq!(auction.status(), AuctionStatus::Sold);
        assert_eq!(auction.winner(), Some(&BidderId::from("alice")));
    }
}
