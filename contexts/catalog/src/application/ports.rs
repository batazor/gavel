use async_trait::async_trait;

use crate::domain::{
    ActualObjectMetadata, Description, DomainError, ImageAttachment, ImageId, Lot, LotId, LotImage,
    ObjectMetadata, PrincipalId, Title,
};

use super::{ApplicationError, UploadIntentResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortError {
    Timeout,
    Unavailable,
    NotFound,
    Internal,
}

impl PortError {
    pub const fn dependency(self, name: &'static str) -> ApplicationError {
        match self {
            Self::Timeout | Self::Unavailable => {
                ApplicationError::DependencyUnavailable { dependency: name }
            }
            Self::NotFound => ApplicationError::NotFound,
            Self::Internal => ApplicationError::Internal,
        }
    }
}

pub trait Clock: Send + Sync {
    fn now_millis(&self) -> u64;
}

pub trait IdGenerator: Send + Sync {
    fn image_id(&self) -> Result<ImageId, PortError>;
    fn object_key(&self, image_id: &ImageId) -> Result<String, PortError>;
    fn event_id(&self) -> Result<String, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresignPutRequest {
    pub object_key: String,
    pub expected: ObjectMetadata,
    pub expires_at_millis: u64,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadObjectRequest {
    pub object_key: String,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteObjectRequest {
    pub object_key: String,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresignedPut {
    pub url: String,
    pub required_headers: Vec<(String, String)>,
}

#[async_trait]
pub trait ObjectStorage: Send + Sync {
    async fn presign_put(&self, request: PresignPutRequest) -> Result<PresignedPut, PortError>;
    async fn head_object(
        &self,
        request: HeadObjectRequest,
    ) -> Result<ActualObjectMetadata, PortError>;
    async fn delete_object(&self, request: DeleteObjectRequest) -> Result<(), PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadClaim {
    pub caller_scope: String,
    pub idempotency_key: String,
    pub fingerprint: String,
    pub image: LotImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadClaimOutcome {
    Created(LotImage),
    PendingReplay(LotImage),
    CompletedReplay(UploadIntentResult),
    Conflict,
    InProgress,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupLease {
    pub image: LotImage,
    pub token: String,
}

#[async_trait]
pub trait MediaRepository: Send + Sync {
    async fn claim_upload(&self, claim: UploadClaim) -> Result<UploadClaimOutcome, PortError>;
    async fn complete_upload_request(
        &self,
        caller_scope: &str,
        idempotency_key: &str,
        result: &UploadIntentResult,
    ) -> Result<(), PortError>;
    async fn image(&self, image_id: &ImageId) -> Result<Option<LotImage>, PortError>;
    async fn save_verified(&self, image: &LotImage) -> Result<(), PortError>;
    async fn mark_for_cleanup(&self, image: &LotImage) -> Result<(), PortError>;
    async fn lease_cleanup(
        &self,
        worker: &str,
        now_millis: u64,
        ready_ttl_millis: u64,
        lease_millis: u64,
        limit: usize,
    ) -> Result<Vec<CleanupLease>, PortError>;
    async fn complete_cleanup(&self, image_id: &ImageId, token: &str) -> Result<(), PortError>;
    async fn release_cleanup(&self, image_id: &ImageId, token: &str) -> Result<(), PortError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaOperation {
    RequestUpload,
    CompleteUpload,
    Cleanup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaOutcome {
    Success,
    Replay,
    Invalid,
    Conflict,
    Missing,
    DependencyFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MediaObservation {
    pub operation: MediaOperation,
    pub outcome: MediaOutcome,
    pub retryable: bool,
}

pub trait MediaTelemetry: Send + Sync {
    fn record(&self, observation: MediaObservation);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandReceiptClaim {
    pub command: &'static str,
    pub caller_scope: String,
    pub idempotency_key: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandReceiptOutcome {
    Acquired { claim_token: String },
    Replay { result_json: Vec<u8> },
    Conflict,
    InProgress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptCompletion {
    Stored,
    LostClaim,
}

#[async_trait]
pub trait CommandReceiptStore: Send + Sync {
    async fn claim(&self, claim: CommandReceiptClaim) -> Result<CommandReceiptOutcome, PortError>;
    async fn complete(
        &self,
        claim_token: &str,
        result_json: &[u8],
    ) -> Result<ReceiptCompletion, PortError>;
    async fn release_retry(&self, claim_token: &str) -> Result<(), PortError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    CreateLot,
    EditLot,
    ExecuteWithdraw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionCheck {
    pub principal: PrincipalId,
    pub permission: Permission,
    pub resource_id: Option<String>,
    pub consistency_token: Option<String>,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionDecision {
    pub allowed: bool,
    pub checked_at_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServicePermissionCheck {
    pub workload: super::WorkloadIdentity,
    pub permission: Permission,
    pub resource_id: String,
    pub consistency_token: Option<String>,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LotOwnerGrant {
    pub lot_id: String,
    pub seller: PrincipalId,
    pub idempotency_key: String,
    pub consistency_token: Option<String>,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationRevision {
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteLotOwnerGrant {
    pub lot_id: String,
    pub seller: PrincipalId,
    pub idempotency_key: String,
    pub timeout_millis: u64,
}

#[async_trait]
pub trait Authorization: Send + Sync {
    async fn check(&self, request: PermissionCheck) -> Result<PermissionDecision, PortError>;
    async fn check_service(
        &self,
        request: ServicePermissionCheck,
    ) -> Result<PermissionDecision, PortError>;
    async fn touch_lot_owner(
        &self,
        request: LotOwnerGrant,
    ) -> Result<AuthorizationRevision, PortError>;
    async fn delete_lot_owner(
        &self,
        _request: DeleteLotOwnerGrant,
    ) -> Result<AuthorizationRevision, PortError> {
        Err(PortError::Internal)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogEvent {
    LotListed {
        event_id: String,
        correlation_id: String,
        occurred_at_millis: u64,
    },
    LotUpdated {
        event_id: String,
        correlation_id: String,
        occurred_at_millis: u64,
    },
    LotWithdrawn {
        event_id: String,
        correlation_id: String,
        occurred_at_millis: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptIdentity {
    pub caller_scope: String,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLotCommit {
    pub lot: Lot,
    pub attachments: Vec<ImageAttachment>,
    pub event: CatalogEvent,
    pub result: super::LotCommandResult,
    pub receipt: ReceiptIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateLotCommit {
    pub lot_id: LotId,
    pub seller: PrincipalId,
    pub expected_version: u64,
    pub title: Option<Title>,
    pub description: Option<Description>,
    pub event: CatalogEvent,
    pub receipt: ReceiptIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawLotCommit {
    pub lot_id: LotId,
    pub event: CatalogEvent,
    pub checked_at_token: String,
    pub receipt: ReceiptIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogCommitError {
    Domain(DomainError),
    Conflict,
    Port(PortError),
}

#[async_trait]
pub trait CatalogUnitOfWork: Send + Sync {
    async fn create_lot(
        &self,
        commit: ListLotCommit,
    ) -> Result<super::LotCommandResult, CatalogCommitError>;
    async fn update_lot(
        &self,
        commit: UpdateLotCommit,
    ) -> Result<super::LotCommandResult, CatalogCommitError>;
    async fn withdraw_lot(
        &self,
        commit: WithdrawLotCommit,
    ) -> Result<super::LotMutationResult, CatalogCommitError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreezeFromAuction {
    pub event_id: String,
    pub payload_fingerprint: String,
    pub lot_id: LotId,
    pub source_version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeInboxOutcome {
    Applied,
    Duplicate,
    Stale,
    Quarantined,
}

#[async_trait]
pub trait AuctionEventInbox: Send + Sync {
    async fn freeze_from_first_bid(
        &self,
        event: FreezeFromAuction,
    ) -> Result<FreezeInboxOutcome, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantReconciliationLease {
    pub operation_id: String,
    pub lot_id: LotId,
    pub seller: PrincipalId,
    pub delete_idempotency_key: String,
    pub not_before_millis: u64,
    pub lease_token: String,
}

#[async_trait]
pub trait GrantReconciliationQueue: Send + Sync {
    async fn lease_due(
        &self,
        worker: &str,
        now_millis: u64,
        lease_millis: u64,
        limit: usize,
    ) -> Result<Vec<GrantReconciliationLease>, PortError>;
    async fn lot_exists(&self, lot_id: &LotId) -> Result<bool, PortError>;
    async fn complete(&self, operation_id: &str, lease_token: &str) -> Result<(), PortError>;
    async fn release(&self, operation_id: &str, lease_token: &str) -> Result<(), PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LotQuery {
    pub first: usize,
    pub after: Option<String>,
}

#[async_trait]
pub trait CatalogReader: Send + Sync {
    async fn lot(&self, lot_id: &LotId) -> Result<Option<super::PublicLot>, ApplicationError>;
    async fn lots(&self, query: LotQuery) -> Result<super::LotPage, ApplicationError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxLease {
    pub event_id: String,
    pub payload: Vec<u8>,
    pub lease_token: String,
}

#[async_trait]
pub trait CatalogOutbox: Send + Sync {
    async fn lease(
        &self,
        worker: &str,
        now_millis: u64,
        lease_millis: u64,
        limit: usize,
    ) -> Result<Vec<OutboxLease>, PortError>;
    async fn mark_published(&self, event_id: &str, lease_token: &str) -> Result<(), PortError>;
    async fn release(&self, event_id: &str, lease_token: &str) -> Result<(), PortError>;
    async fn delete_published_before(&self, cutoff_millis: u64) -> Result<u64, PortError>;
}
