//! Pure provider-neutral Payments domain model.

mod attempt;
mod instrument;
mod value;

pub use attempt::{AttemptTransition, PaymentAttempt, PaymentAttemptState};
pub use instrument::{
    DebitAuthorization, InstrumentTransition, PaymentInstrument, PaymentInstrumentState,
};
pub use value::{
    AuctionId, CandidateId, Currency, DomainError, EmailAddress, Money, PaymentAttemptId,
    PaymentInstrumentId, PrincipalId, ProviderReference, RedactedDisplay, SettlementRound,
    TermsVersion,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    PaymentInstrumentReady {
        event_id: String,
        instrument_id: PaymentInstrumentId,
        owner_id: PrincipalId,
        version: u64,
        occurred_at_millis: u64,
    },
    PaymentInstrumentRevoked {
        event_id: String,
        instrument_id: PaymentInstrumentId,
        owner_id: PrincipalId,
        version: u64,
        occurred_at_millis: u64,
    },
    PaymentAttemptStarted {
        event_id: String,
        attempt_id: PaymentAttemptId,
        auction_id: AuctionId,
        settlement_round: SettlementRound,
        candidate_id: CandidateId,
        amount: Money,
        version: u64,
        occurred_at_millis: u64,
    },
    PaymentRecoveryRequired {
        event_id: String,
        attempt_id: PaymentAttemptId,
        auction_id: AuctionId,
        settlement_round: SettlementRound,
        version: u64,
        occurred_at_millis: u64,
    },
    PaymentSucceeded {
        event_id: String,
        attempt_id: PaymentAttemptId,
        auction_id: AuctionId,
        settlement_round: SettlementRound,
        version: u64,
        occurred_at_millis: u64,
    },
    PaymentExpired {
        event_id: String,
        attempt_id: PaymentAttemptId,
        auction_id: AuctionId,
        settlement_round: SettlementRound,
        version: u64,
        occurred_at_millis: u64,
    },
}
