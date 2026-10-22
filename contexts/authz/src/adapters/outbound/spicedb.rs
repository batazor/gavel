use std::{fmt, time::Duration};

use async_trait::async_trait;
use tonic::{
    Code, Request,
    metadata::{Ascii, MetadataValue},
    service::{Interceptor, interceptor::InterceptedService},
    transport::{Channel, Endpoint},
};
use tower::ServiceExt;

use crate::{
    application::{
        PermissionCheck, PolicyBulkCheckResult, PolicyCheckResult, PolicyDecision, PolicyEngine,
        PolicyEngineError, PolicyWriteResult, Relationship,
    },
    spicedb_rpc::{self as rpc, permissions_service_client::PermissionsServiceClient},
};

type Client = PermissionsServiceClient<InterceptedService<Channel, CredentialInterceptor>>;

#[derive(Clone)]
struct CredentialInterceptor {
    authorization: MetadataValue<Ascii>,
}

impl fmt::Debug for CredentialInterceptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialInterceptor")
            .field("authorization", &"[REDACTED]")
            .finish()
    }
}

impl Interceptor for CredentialInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, tonic::Status> {
        request
            .metadata_mut()
            .insert("authorization", self.authorization.clone());
        Ok(request)
    }
}

#[derive(Clone)]
pub struct SpiceDbAdapter {
    client: Client,
    channel: Channel,
    timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum SpiceDbAdapterError {
    #[error("SpiceDB endpoint is invalid")]
    InvalidEndpoint,
    #[error("SpiceDB credential is invalid")]
    InvalidCredential,
    #[error("SpiceDB connection failed")]
    Connection,
    #[error("SpiceDB timeout must be positive")]
    InvalidTimeout,
}

impl SpiceDbAdapter {
    pub async fn connect(
        endpoint: &str,
        credential: &str,
        timeout: Duration,
    ) -> Result<Self, SpiceDbAdapterError> {
        if timeout.is_zero() {
            return Err(SpiceDbAdapterError::InvalidTimeout);
        }
        let endpoint = Endpoint::from_shared(endpoint.to_owned())
            .map_err(|_| SpiceDbAdapterError::InvalidEndpoint)?;
        let channel = endpoint
            .connect()
            .await
            .map_err(|_| SpiceDbAdapterError::Connection)?;
        Self::from_channel(channel, credential, timeout)
    }

    pub fn from_channel(
        channel: Channel,
        credential: &str,
        timeout: Duration,
    ) -> Result<Self, SpiceDbAdapterError> {
        if timeout.is_zero() {
            return Err(SpiceDbAdapterError::InvalidTimeout);
        }
        if credential.is_empty() {
            return Err(SpiceDbAdapterError::InvalidCredential);
        }
        let authorization = format!("Bearer {credential}")
            .parse()
            .map_err(|_| SpiceDbAdapterError::InvalidCredential)?;
        let client = PermissionsServiceClient::with_interceptor(
            channel.clone(),
            CredentialInterceptor { authorization },
        );
        Ok(Self {
            client,
            channel,
            timeout,
        })
    }

    pub async fn ready(&self) -> Result<(), PolicyEngineError> {
        tokio::time::timeout(self.timeout, self.channel.clone().ready())
            .await
            .map_err(|_| PolicyEngineError::Timeout)?
            .map(|_| ())
            .map_err(|_| PolicyEngineError::Unavailable)
    }
}

#[async_trait]
impl PolicyEngine for SpiceDbAdapter {
    async fn check_permission(
        &self,
        check: &PermissionCheck,
        consistency_token: Option<&str>,
    ) -> Result<PolicyCheckResult, PolicyEngineError> {
        let mut request = Request::new(rpc::CheckPermissionRequest {
            consistency: consistency(consistency_token),
            resource: Some(object(
                &check.resource.resource_type,
                &check.resource.resource_id,
            )),
            permission: check.permission.clone(),
            subject: Some(subject(
                &check.principal.principal_type,
                &check.principal.principal_id,
            )),
        });
        request.set_timeout(self.timeout);
        let response = self
            .client
            .clone()
            .check_permission(request)
            .await
            .map_err(map_status)?
            .into_inner();
        Ok(PolicyCheckResult {
            decision: map_decision(response.permissionship)?,
            checked_at_token: required_token(response.checked_at)?,
        })
    }

    async fn bulk_check_permissions(
        &self,
        checks: &[PermissionCheck],
        consistency_token: Option<&str>,
    ) -> Result<PolicyBulkCheckResult, PolicyEngineError> {
        let items = checks.iter().map(bulk_item).collect::<Vec<_>>();
        let mut request = Request::new(rpc::CheckBulkPermissionsRequest {
            consistency: consistency(consistency_token),
            items: items.clone(),
        });
        request.set_timeout(self.timeout);
        let response = self
            .client
            .clone()
            .check_bulk_permissions(request)
            .await
            .map_err(map_status)?
            .into_inner();
        if response.pairs.len() != items.len() {
            return Err(PolicyEngineError::InvalidResponse);
        }
        let decisions = response
            .pairs
            .into_iter()
            .zip(items)
            .map(|(pair, expected)| {
                if pair.request.as_ref() != Some(&expected) {
                    return Err(PolicyEngineError::InvalidResponse);
                }
                match pair.response {
                    Some(rpc::check_bulk_permissions_pair::Response::Item(item)) => {
                        map_decision(item.permissionship)
                    }
                    None => Err(PolicyEngineError::InvalidResponse),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PolicyBulkCheckResult {
            decisions,
            checked_at_token: required_token(response.checked_at)?,
        })
    }

    async fn write_relationships(
        &self,
        updates: &[Relationship],
        deletions: &[Relationship],
    ) -> Result<PolicyWriteResult, PolicyEngineError> {
        let updates = updates
            .iter()
            .map(|relationship| {
                relationship_update(relationship, rpc::relationship_update::Operation::Touch)
            })
            .chain(deletions.iter().map(|relationship| {
                relationship_update(relationship, rpc::relationship_update::Operation::Delete)
            }))
            .collect();
        let mut request = Request::new(rpc::WriteRelationshipsRequest { updates });
        request.set_timeout(self.timeout);
        let response = self
            .client
            .clone()
            .write_relationships(request)
            .await
            .map_err(map_status)?
            .into_inner();
        Ok(PolicyWriteResult {
            written_at_token: required_token(response.written_at)?,
        })
    }
}

fn consistency(token: Option<&str>) -> Option<rpc::Consistency> {
    token.map(|token| rpc::Consistency {
        requirement: Some(rpc::consistency::Requirement::AtLeastAsFresh(
            rpc::ZedToken {
                token: token.to_owned(),
            },
        )),
    })
}

fn object(object_type: &str, object_id: &str) -> rpc::ObjectReference {
    rpc::ObjectReference {
        object_type: object_type.to_owned(),
        object_id: object_id.to_owned(),
    }
}

fn subject(object_type: &str, object_id: &str) -> rpc::SubjectReference {
    rpc::SubjectReference {
        object: Some(object(object_type, object_id)),
        optional_relation: String::new(),
    }
}

fn bulk_item(check: &PermissionCheck) -> rpc::CheckBulkPermissionsRequestItem {
    rpc::CheckBulkPermissionsRequestItem {
        resource: Some(object(
            &check.resource.resource_type,
            &check.resource.resource_id,
        )),
        permission: check.permission.clone(),
        subject: Some(subject(
            &check.principal.principal_type,
            &check.principal.principal_id,
        )),
    }
}

fn relationship_update(
    relationship: &Relationship,
    operation: rpc::relationship_update::Operation,
) -> rpc::RelationshipUpdate {
    rpc::RelationshipUpdate {
        operation: operation as i32,
        relationship: Some(rpc::Relationship {
            resource: Some(object(
                &relationship.resource.resource_type,
                &relationship.resource.resource_id,
            )),
            relation: relationship.relation.clone(),
            subject: Some(subject(
                &relationship.subject.principal_type,
                &relationship.subject.principal_id,
            )),
        }),
    }
}

fn map_decision(value: i32) -> Result<PolicyDecision, PolicyEngineError> {
    match rpc::check_permission_response::Permissionship::try_from(value) {
        Ok(rpc::check_permission_response::Permissionship::HasPermission) => {
            Ok(PolicyDecision::Allowed)
        }
        Ok(rpc::check_permission_response::Permissionship::NoPermission) => {
            Ok(PolicyDecision::Denied)
        }
        _ => Err(PolicyEngineError::InvalidResponse),
    }
}

fn required_token(token: Option<rpc::ZedToken>) -> Result<String, PolicyEngineError> {
    token
        .map(|token| token.token)
        .filter(|token| !token.is_empty())
        .ok_or(PolicyEngineError::InvalidResponse)
}

fn map_status(status: tonic::Status) -> PolicyEngineError {
    match status.code() {
        Code::InvalidArgument => PolicyEngineError::InvalidArgument,
        Code::DeadlineExceeded | Code::Cancelled => PolicyEngineError::Timeout,
        Code::Unavailable => PolicyEngineError::Unavailable,
        _ => PolicyEngineError::InvalidResponse,
    }
}
