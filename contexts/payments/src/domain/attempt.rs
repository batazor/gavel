use serde::{Deserialize, Serialize};

use super::{
    AuctionId, CandidateId, DomainError, Money, PaymentAttemptId, ProviderReference,
    SettlementRound,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentAttemptState {
    Pending,
    Processing,
    RecoveryRequired,
    Succeeded,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptTransition {
    Processing { version: u64 },
    RecoveryRequired { version: u64 },
    Succeeded { version: u64 },
    Expired { version: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentAttempt {
    id: PaymentAttemptId,
    auction_id: AuctionId,
    settlement_round: SettlementRound,
    candidate_id: CandidateId,
    amount: Money,
    state: PaymentAttemptState,
    provider_operation_ref: Option<ProviderReference>,
    recovery_url: Option<String>,
    version: u64,
    updated_at_millis: u64,
}

impl PaymentAttempt {
    pub fn pending(
        id: PaymentAttemptId,
        auction_id: AuctionId,
        settlement_round: SettlementRound,
        candidate_id: CandidateId,
        amount: Money,
        now: u64,
    ) -> Self {
        Self {
            id,
            auction_id,
            settlement_round,
            candidate_id,
            amount,
            state: PaymentAttemptState::Pending,
            provider_operation_ref: None,
            recovery_url: None,
            version: 1,
            updated_at_millis: now,
        }
    }
    fn transition(&mut self, target: PaymentAttemptState, now: u64) -> Result<u64, DomainError> {
        let allowed = matches!(
            (self.state, target),
            (
                PaymentAttemptState::Pending,
                PaymentAttemptState::Processing
                    | PaymentAttemptState::RecoveryRequired
                    | PaymentAttemptState::Succeeded
            ) | (
                PaymentAttemptState::Processing,
                PaymentAttemptState::RecoveryRequired
                    | PaymentAttemptState::Succeeded
                    | PaymentAttemptState::Expired
            ) | (
                PaymentAttemptState::RecoveryRequired,
                PaymentAttemptState::Processing
                    | PaymentAttemptState::Succeeded
                    | PaymentAttemptState::Expired
            )
        );
        if !allowed {
            return Err(DomainError::InvalidTransition);
        }
        self.version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::VersionOverflow)?;
        self.state = target;
        self.updated_at_millis = now;
        Ok(self.version)
    }
    pub fn mark_processing(
        &mut self,
        operation_ref: ProviderReference,
        now: u64,
    ) -> Result<AttemptTransition, DomainError> {
        let version = self.transition(PaymentAttemptState::Processing, now)?;
        self.provider_operation_ref = Some(operation_ref);
        self.recovery_url = None;
        Ok(AttemptTransition::Processing { version })
    }
    pub fn require_recovery(
        &mut self,
        operation_ref: ProviderReference,
        recovery_url: String,
        now: u64,
    ) -> Result<AttemptTransition, DomainError> {
        if !recovery_url.starts_with("https://") {
            return Err(DomainError::InvalidFormat);
        }
        let version = self.transition(PaymentAttemptState::RecoveryRequired, now)?;
        self.provider_operation_ref = Some(operation_ref);
        self.recovery_url = Some(recovery_url);
        Ok(AttemptTransition::RecoveryRequired { version })
    }
    pub fn succeed(
        &mut self,
        operation_ref: ProviderReference,
        now: u64,
    ) -> Result<AttemptTransition, DomainError> {
        let version = self.transition(PaymentAttemptState::Succeeded, now)?;
        self.provider_operation_ref = Some(operation_ref);
        self.recovery_url = None;
        Ok(AttemptTransition::Succeeded { version })
    }
    pub fn expire(&mut self, now: u64) -> Result<AttemptTransition, DomainError> {
        let version = self.transition(PaymentAttemptState::Expired, now)?;
        self.recovery_url = None;
        Ok(AttemptTransition::Expired { version })
    }
    pub fn id(&self) -> &PaymentAttemptId {
        &self.id
    }
    pub fn auction_id(&self) -> &AuctionId {
        &self.auction_id
    }
    pub const fn settlement_round(&self) -> SettlementRound {
        self.settlement_round
    }
    pub fn candidate_id(&self) -> &CandidateId {
        &self.candidate_id
    }
    pub fn amount(&self) -> &Money {
        &self.amount
    }
    pub const fn state(&self) -> PaymentAttemptState {
        self.state
    }
    pub const fn version(&self) -> u64 {
        self.version
    }
    pub fn provider_operation_ref(&self) -> Option<&ProviderReference> {
        self.provider_operation_ref.as_ref()
    }
    pub fn recovery_url(&self) -> Option<&str> {
        self.recovery_url.as_deref()
    }
}
