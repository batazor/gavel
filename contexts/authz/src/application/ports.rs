//! Ports implemented by policy and receipt adapters.

use async_trait::async_trait;

use super::{MutationResult, PermissionCheck, Relationship, WorkloadIdentity};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyDecision {
    Allowed,
    Denied,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyCheckResult {
    pub decision: PolicyDecision,
    pub checked_at_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyBulkCheckResult {
    pub decisions: Vec<PolicyDecision>,
    pub checked_at_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyWriteResult {
    pub written_at_token: String,
}

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum PolicyEngineError {
    #[error("policy request was invalid")]
    InvalidArgument,
    #[error("policy engine timed out")]
    Timeout,
    #[error("policy engine is unavailable")]
    Unavailable,
    #[error("policy engine returned an invalid response")]
    InvalidResponse,
}

#[async_trait]
pub trait PolicyEngine: Send + Sync {
    async fn check_permission(
        &self,
        check: &PermissionCheck,
        consistency_token: Option<&str>,
    ) -> Result<PolicyCheckResult, PolicyEngineError>;

    async fn bulk_check_permissions(
        &self,
        checks: &[PermissionCheck],
        consistency_token: Option<&str>,
    ) -> Result<PolicyBulkCheckResult, PolicyEngineError>;

    async fn write_relationships(
        &self,
        _updates: &[Relationship],
        _deletions: &[Relationship],
    ) -> Result<PolicyWriteResult, PolicyEngineError> {
        Err(PolicyEngineError::InvalidResponse)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationOperation {
    RegisterPrincipal,
    WriteRelationships,
}

impl MutationOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RegisterPrincipal => "register_principal",
            Self::WriteRelationships => "write_relationships",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptClaimRequest {
    pub caller: WorkloadIdentity,
    pub idempotency_key: String,
    pub operation: MutationOperation,
    pub fingerprint: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptLease {
    pub caller: WorkloadIdentity,
    pub idempotency_key: String,
    pub fingerprint: [u8; 32],
    pub lease_token: String,
    pub attempt: u32,
    pub recovered: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptClaim {
    Acquired(ReceiptLease),
    Replay(MutationResult),
    Conflict,
    InProgress,
}

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReceiptStoreError {
    #[error("receipt store is unavailable")]
    Unavailable,
    #[error("receipt lease is no longer valid")]
    InvalidLease,
    #[error("receipt store failed")]
    Internal,
}

#[async_trait]
pub trait CommandReceiptStore: Send + Sync {
    async fn claim(&self, request: ReceiptClaimRequest) -> Result<ReceiptClaim, ReceiptStoreError>;

    async fn complete(
        &self,
        lease: &ReceiptLease,
        result: &MutationResult,
    ) -> Result<(), ReceiptStoreError>;
}
