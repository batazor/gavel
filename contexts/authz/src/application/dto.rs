//! Stable, transport-independent values shared by authorization use cases.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkloadIdentity {
    Bootstrap,
    Bff,
    Auction,
    Catalog,
    Payments,
    Notify,
    Processes,
}

impl WorkloadIdentity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Bff => "bff",
            Self::Auction => "auction",
            Self::Catalog => "catalog",
            Self::Payments => "payments",
            Self::Notify => "notify",
            Self::Processes => "processes",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourceReference {
    pub resource_type: String,
    pub resource_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PrincipalReference {
    pub principal_type: String,
    pub principal_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PermissionCheck {
    pub resource: ResourceReference,
    pub permission: String,
    pub principal: PrincipalReference,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckPermissionInput {
    pub caller: WorkloadIdentity,
    pub check: PermissionCheck,
    pub consistency_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkCheckPermissionsInput {
    pub caller: WorkloadIdentity,
    pub checks: Vec<PermissionCheck>,
    pub consistency_token: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Allowed,
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionResult {
    pub decision: PermissionDecision,
    pub checked_at_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkPermissionResult {
    pub decisions: Vec<PermissionDecision>,
    pub checked_at_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Relationship {
    pub resource: ResourceReference,
    pub relation: String,
    pub subject: PrincipalReference,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterPrincipalInput {
    pub caller: WorkloadIdentity,
    pub issuer: String,
    pub subject: String,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrincipalRegistered {
    pub principal_id: String,
    pub written_at_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteRelationshipsInput {
    pub caller: WorkloadIdentity,
    pub updates: Vec<Relationship>,
    pub deletions: Vec<Relationship>,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RelationshipsWritten {
    pub written_at_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DerivedPrincipal {
    pub principal_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationResult {
    PrincipalRegistered(PrincipalRegistered),
    RelationshipsWritten(RelationshipsWritten),
}
