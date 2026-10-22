use async_trait::async_trait;

use crate::domain::{
    PaymentAttempt, PaymentAttemptId, PaymentInstrument, PrincipalId, ProviderReference,
};

use super::ApplicationError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    RegisterInstrument,
    ManageInstrument,
    ViewInstrument,
    ViewAttempt,
    ContinuePayment,
    ProcessPayment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub principal: String,
    pub permission: Permission,
    pub resource_id: Option<String>,
    pub consistency_token: Option<String>,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationDecision {
    pub allowed: bool,
    pub checked_at_token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortError {
    Timeout,
    Unavailable,
    Conflict,
    NotFound,
    Internal,
}

impl From<PortError> for ApplicationError {
    fn from(error: PortError) -> Self {
        match error {
            PortError::Timeout | PortError::Unavailable => Self::DependencyUnavailable,
            PortError::Conflict => Self::IdempotencyConflict,
            PortError::NotFound => Self::NotFound,
            PortError::Internal => Self::Internal,
        }
    }
}

#[async_trait]
pub trait Authorization: Send + Sync {
    async fn check(
        &self,
        request: AuthorizationRequest,
    ) -> Result<AuthorizationDecision, PortError>;
    async fn touch_owner_grant(
        &self,
        instrument_id: &str,
        owner: &PrincipalId,
        idempotency_key: &str,
        timeout_millis: u64,
    ) -> Result<String, PortError>;
    async fn delete_owner_grant(
        &self,
        instrument_id: &str,
        owner: &PrincipalId,
        idempotency_key: &str,
        timeout_millis: u64,
    ) -> Result<(), PortError>;
}

pub trait Clock: Send + Sync {
    fn now_millis(&self) -> u64;
}
pub trait IdGenerator: Send + Sync {
    fn instrument_id(&self) -> Result<crate::domain::PaymentInstrumentId, PortError>;
    fn attempt_id(&self) -> Result<PaymentAttemptId, PortError>;
    fn event_id(&self) -> Result<String, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareInstrumentRequest {
    pub provider_key: String,
    pub recovery_email: String,
    pub timeout_millis: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedInstrument {
    pub provider_url: String,
    pub provider_session_ref: ProviderReference,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChargeRequest {
    pub provider_key: String,
    pub customer_ref: ProviderReference,
    pub instrument_ref: ProviderReference,
    pub amount_minor_units: u64,
    pub currency_code: String,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderOutcome {
    Processing {
        operation_ref: ProviderReference,
    },
    RecoveryRequired {
        operation_ref: ProviderReference,
        recovery_url: String,
    },
    Succeeded {
        operation_ref: ProviderReference,
    },
    Declined,
}

#[async_trait]
pub trait ProviderGateway: Send + Sync {
    async fn prepare(
        &self,
        request: PrepareInstrumentRequest,
    ) -> Result<PreparedInstrument, PortError>;
    async fn charge(&self, request: ChargeRequest) -> Result<ProviderOutcome, PortError>;
    async fn continue_payment(
        &self,
        operation_ref: &ProviderReference,
        timeout_millis: u64,
    ) -> Result<ProviderOutcome, PortError>;
    async fn query_or_cancel(
        &self,
        operation_ref: &ProviderReference,
        timeout_millis: u64,
    ) -> Result<ProviderOutcome, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit<T> {
    pub value: T,
    pub events: Vec<crate::domain::DomainEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitConflict {
    Version,
    AttemptForRound,
    SuccessForAuction,
}

#[async_trait]
pub trait PaymentsUnitOfWork: Send + Sync {
    async fn instrument_for_owner(
        &self,
        owner: &PrincipalId,
    ) -> Result<Option<PaymentInstrument>, PortError>;
    async fn instrument(
        &self,
        id: &crate::domain::PaymentInstrumentId,
    ) -> Result<Option<PaymentInstrument>, PortError>;
    async fn instrument_by_session_ref(
        &self,
        session_ref: &ProviderReference,
    ) -> Result<Option<PaymentInstrument>, PortError>;
    async fn save_instrument(
        &self,
        commit: Commit<PaymentInstrument>,
        expected_version: Option<u64>,
    ) -> Result<(), CommitConflict>;
    async fn attempt(&self, id: &PaymentAttemptId) -> Result<Option<PaymentAttempt>, PortError>;
    async fn attempt_for_round(
        &self,
        auction_id: &str,
        round: u32,
    ) -> Result<Option<PaymentAttempt>, PortError>;
    async fn attempt_by_operation_ref(
        &self,
        operation_ref: &ProviderReference,
    ) -> Result<Option<PaymentAttempt>, PortError>;
    async fn save_attempt(
        &self,
        commit: Commit<PaymentAttempt>,
        expected_version: Option<u64>,
    ) -> Result<(), CommitConflict>;
    async fn stale_attempts(
        &self,
        before_millis: u64,
        limit: usize,
    ) -> Result<Vec<PaymentAttempt>, PortError>;
    async fn revoked_instruments(&self, limit: usize) -> Result<Vec<PaymentInstrument>, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptClaim {
    pub scope: String,
    pub command: &'static str,
    pub key_digest: String,
    pub fingerprint: String,
    pub lease_until_millis: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptClaimResult {
    Acquired { token: String },
    Replay(Vec<u8>),
    Conflict,
    InProgress,
}

#[async_trait]
pub trait ReceiptStore: Send + Sync {
    async fn claim(&self, claim: ReceiptClaim) -> Result<ReceiptClaimResult, PortError>;
    async fn complete(&self, token: &str, result: &[u8]) -> Result<(), PortError>;
    async fn release(&self, token: &str) -> Result<(), PortError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackClaim {
    Acquired,
    Replay,
    InProgress,
    Conflict,
}

#[async_trait]
pub trait CallbackInbox: Send + Sync {
    async fn claim(
        &self,
        provider: &str,
        event_ref_digest: &str,
        fingerprint: &str,
        lease_until_millis: u64,
    ) -> Result<CallbackClaim, PortError>;
    async fn complete(&self, provider: &str, event_ref_digest: &str) -> Result<(), PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedProviderEvent {
    pub provider: &'static str,
    pub event_ref: ProviderReference,
    pub event_ref_digest: String,
    pub fingerprint: String,
    pub occurred_at_millis: u64,
    pub outcome: VerifiedProviderOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifiedProviderOutcome {
    InstrumentReady {
        session_ref: ProviderReference,
        customer_ref: ProviderReference,
        instrument_ref: ProviderReference,
        display: crate::domain::RedactedDisplay,
    },
    AttemptProcessing {
        operation_ref: ProviderReference,
    },
    AttemptRecoveryRequired {
        operation_ref: ProviderReference,
        recovery_url: String,
    },
    AttemptSucceeded {
        operation_ref: ProviderReference,
    },
}

pub trait Telemetry: Send + Sync {
    fn command(&self, command: &'static str, outcome: &'static str);
}
