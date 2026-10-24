//! Owner-effect ports. Implementations may perform I/O; workflows depend only on these DTOs.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::workflows::{
    AuctionId, CandidateId, Money, PaymentAttemptId, ProcessError, SettlementAttemptStatus,
    SettlementRound,
};

mod temporal;
pub use temporal::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallContext {
    pub correlation_id: String,
    pub causation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseAuctionIntent {
    pub auction_id: AuctionId,
    pub observed_at_millis: u64,
    pub idempotency_key: String,
    pub call: CallContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForfeitCandidateIntent {
    pub auction_id: AuctionId,
    pub candidate_id: CandidateId,
    pub round: SettlementRound,
    pub idempotency_key: String,
    pub call: CallContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmWinnerIntent {
    pub auction_id: AuctionId,
    pub candidate_id: CandidateId,
    pub payment_id: String,
    pub round: SettlementRound,
    pub idempotency_key: String,
    pub call: CallContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuctionMutationOutcome {
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptPaymentIntent {
    pub auction_id: AuctionId,
    pub round: SettlementRound,
    pub candidate_id: CandidateId,
    pub amount: Money,
    pub idempotency_key: String,
    pub correlation_id: String,
    pub consistency_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementAttemptRead {
    pub attempt_id: PaymentAttemptId,
    pub expected_auction_id: AuctionId,
    pub expected_round: SettlementRound,
    pub correlation_id: String,
    pub consistency_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpirePaymentIntent {
    pub attempt_id: PaymentAttemptId,
    pub auction_id: AuctionId,
    pub round: SettlementRound,
    pub candidate_id: CandidateId,
    pub idempotency_key: String,
    pub correlation_id: String,
    pub consistency_token: String,
}

/// Safe workflow-facing payment state. Recovery material is intentionally absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementAttemptSnapshot {
    pub attempt_id: PaymentAttemptId,
    pub candidate_id: CandidateId,
    pub status: SettlementAttemptStatus,
    pub version: u64,
    pub checked_at_token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationKind {
    SettlementCandidate,
    SettlementCandidateFailed,
    PaymentRecoveryRequired,
    WinnerConfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyIntent {
    pub auction_id: AuctionId,
    pub round: SettlementRound,
    pub recipient_id: CandidateId,
    pub kind: NotificationKind,
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryNotificationIntent {
    pub attempt: SettlementAttemptRead,
    pub recipient_id: CandidateId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutbidNotificationIntent {
    pub auction_id: AuctionId,
    pub aggregate_version: u64,
    pub recipient_id: CandidateId,
    pub correlation_id: String,
}

/// Acknowledgement safe to serialize into Temporal history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAcknowledgement {
    pub notification_id: String,
    pub accepted: bool,
}

#[async_trait]
pub trait AuctionActivities: Send + Sync {
    async fn close_auction(
        &self,
        intent: CloseAuctionIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError>;
    async fn forfeit_settlement_candidate(
        &self,
        intent: ForfeitCandidateIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError>;
    async fn confirm_winner(
        &self,
        intent: ConfirmWinnerIntent,
    ) -> Result<AuctionMutationOutcome, ProcessError>;
}

#[async_trait]
pub trait PaymentsActivities: Send + Sync {
    async fn attempt_payment(
        &self,
        intent: AttemptPaymentIntent,
    ) -> Result<SettlementAttemptSnapshot, ProcessError>;
    async fn get_settlement_attempt(
        &self,
        read: SettlementAttemptRead,
    ) -> Result<SettlementAttemptSnapshot, ProcessError>;
    async fn expire_payment(
        &self,
        intent: ExpirePaymentIntent,
    ) -> Result<SettlementAttemptSnapshot, ProcessError>;
}

#[async_trait]
pub trait NotifyActivities: Send + Sync {
    async fn notify(
        &self,
        intent: NotifyIntent,
    ) -> Result<NotificationAcknowledgement, ProcessError>;
    async fn notify_payment_recovery(
        &self,
        intent: RecoveryNotificationIntent,
    ) -> Result<NotificationAcknowledgement, ProcessError>;
    async fn notify_outbid(
        &self,
        intent: OutbidNotificationIntent,
    ) -> Result<NotificationAcknowledgement, ProcessError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeOwnerActivities;

    #[async_trait]
    impl AuctionActivities for FakeOwnerActivities {
        async fn close_auction(
            &self,
            _: CloseAuctionIntent,
        ) -> Result<AuctionMutationOutcome, ProcessError> {
            Ok(AuctionMutationOutcome { version: 1 })
        }
        async fn forfeit_settlement_candidate(
            &self,
            _: ForfeitCandidateIntent,
        ) -> Result<AuctionMutationOutcome, ProcessError> {
            Ok(AuctionMutationOutcome { version: 2 })
        }
        async fn confirm_winner(
            &self,
            _: ConfirmWinnerIntent,
        ) -> Result<AuctionMutationOutcome, ProcessError> {
            Ok(AuctionMutationOutcome { version: 3 })
        }
    }

    #[async_trait]
    impl PaymentsActivities for FakeOwnerActivities {
        async fn attempt_payment(
            &self,
            _: AttemptPaymentIntent,
        ) -> Result<SettlementAttemptSnapshot, ProcessError> {
            Err(ProcessError::DependencyUnavailable)
        }
        async fn get_settlement_attempt(
            &self,
            _: SettlementAttemptRead,
        ) -> Result<SettlementAttemptSnapshot, ProcessError> {
            Err(ProcessError::DependencyUnavailable)
        }
        async fn expire_payment(
            &self,
            _: ExpirePaymentIntent,
        ) -> Result<SettlementAttemptSnapshot, ProcessError> {
            Err(ProcessError::DependencyUnavailable)
        }
    }

    fn accepts_compile_time_fakes<T: AuctionActivities + PaymentsActivities>(_: &T) {}

    #[test]
    fn ports_accept_framework_free_fakes() {
        accepts_compile_time_fakes(&FakeOwnerActivities);
    }
}
