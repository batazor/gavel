use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::{AuctionEvent, PublicEvent, Timestamp};

use super::{AuctionView, LifecycleResult, PlaceBidResult, Principal};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedAuction {
    pub snapshot: Option<crate::domain::Auction>,
    pub events: Vec<AuctionEvent>,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CommandIdentity {
    pub principal_scope: String,
    pub idempotency_key: String,
    pub request_fingerprint: String,
    pub operation: String,
    pub correlation_id: String,
    pub causation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "result", rename_all = "snake_case")]
pub enum CommandResult {
    Opened { version: u64 },
    PlaceBid(PlaceBidResult),
    Auction(AuctionView),
    Lifecycle(LifecycleResult),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuctionProjection {
    pub view: AuctionView,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciliationOperation {
    pub operation_id: String,
    pub resource_type: String,
    pub resource_id: String,
    pub relation: String,
    pub subject_type: String,
    pub subject_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitRequest {
    pub auction_id: String,
    pub expected_version: u64,
    pub private_events: Vec<AuctionEvent>,
    pub public_events: Vec<PublicEvent>,
    pub command: Option<(CommandIdentity, CommandResult)>,
    pub projection: Option<AuctionProjection>,
    pub reconciliation: Vec<ReconciliationOperation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitOutcome {
    Committed { version: u64 },
    Replayed { result: CommandResult },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandLookup {
    Missing,
    Replay(CommandResult),
    Conflict,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum CommitError {
    #[error("auction stream version changed")]
    VersionConflict,
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("auction persistence is unavailable")]
    Unavailable,
    #[error("invalid atomic commit request")]
    InvalidRequest,
    #[error("auction persistence failed")]
    Internal,
}

#[async_trait]
pub trait AuctionUnitOfWork: Send + Sync {
    async fn load(&self, auction_id: &str) -> Result<Option<LoadedAuction>, CommitError>;
    async fn lookup_command(
        &self,
        identity: &CommandIdentity,
    ) -> Result<CommandLookup, CommitError>;
    async fn commit(&self, request: CommitRequest) -> Result<CommitOutcome, CommitError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionCheck {
    pub principal: Principal,
    pub resource_type: String,
    pub resource_id: String,
    pub permission: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorizationDecision {
    Allowed,
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationshipWrite {
    pub operation: ReconciliationOperation,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum PortError {
    #[error("dependency is unavailable")]
    Unavailable,
    #[error("dependency rejected invalid input")]
    InvalidInput,
    #[error("dependency failed")]
    Internal,
}

#[async_trait]
pub trait Authorization: Send + Sync {
    async fn check(&self, request: PermissionCheck) -> Result<AuthorizationDecision, PortError>;

    async fn write_relationship(&self, request: RelationshipWrite) -> Result<(), PortError>;
}

#[async_trait]
pub trait PaymentReadiness: Send + Sync {
    async fn is_ready(&self, user_id: &str) -> Result<bool, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentReadinessUpdate {
    pub user_id: String,
    pub ready: bool,
    pub source_version: u64,
    pub source_event_id: String,
    pub occurred_at: Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentReadinessUpdateOutcome {
    Applied,
    Duplicate,
    Stale,
}

#[async_trait]
pub trait PaymentReadinessStore: PaymentReadiness {
    async fn apply_update(
        &self,
        update: PaymentReadinessUpdate,
    ) -> Result<PaymentReadinessUpdateOutcome, PortError>;
}

#[async_trait]
pub trait AuctionProjectionReader: Send + Sync {
    async fn auction(&self, auction_id: &str) -> Result<Option<AuctionProjection>, PortError>;
    async fn auction_by_lot_id(&self, lot_id: &str)
    -> Result<Option<AuctionProjection>, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceSnapshot {
    pub auction_id: String,
    pub amount_minor: u64,
    pub currency_code: String,
    pub version: u64,
    pub updated_at_millis: u64,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum CacheError {
    #[error("cache is unavailable")]
    Unavailable,
    #[error("cache value is invalid")]
    InvalidValue,
}

#[async_trait]
pub trait AuctionCache: Send + Sync {
    async fn get(&self, auction_id: &str) -> Result<Option<PriceSnapshot>, CacheError>;
    async fn put_if_newer(&self, snapshot: PriceSnapshot) -> Result<bool, CacheError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InboxOutcome {
    Accepted,
    Duplicate,
    PayloadConflict,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum InboxError {
    #[error("inbox is unavailable")]
    Unavailable,
    #[error("inbox failed")]
    Internal,
}

#[async_trait]
pub trait Inbox: Send + Sync {
    async fn accept(
        &self,
        consumer: &str,
        event_id: &str,
        payload_fingerprint: &str,
    ) -> Result<InboxOutcome, InboxError>;
}

#[async_trait]
pub trait ReconciliationQueue: Send + Sync {
    async fn pending(&self, limit: usize) -> Result<Vec<ReconciliationOperation>, PortError>;
    async fn complete(&self, operation_id: &str) -> Result<(), PortError>;
}
