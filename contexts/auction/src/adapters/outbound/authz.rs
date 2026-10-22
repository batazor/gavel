use std::time::Duration;

use async_trait::async_trait;
use tonic::{Request, transport::Channel};

use crate::{
    application::{
        Authorization, AuthorizationDecision, PermissionCheck, PortError, PrincipalKind,
        RelationshipWrite,
    },
    authz_rpc::{
        CheckPermissionRequest, PermissionCheck as RpcPermissionCheck, PermissionDecision,
        PrincipalReference, Relationship, ResourceReference, WriteRelationshipsRequest,
        authz_service_client::AuthzServiceClient, write_relationships_response::Outcome,
    },
};

#[derive(Clone)]
pub struct AuthzGrpcAdapter {
    client: AuthzServiceClient<Channel>,
    timeout: Duration,
}

impl AuthzGrpcAdapter {
    pub fn new(channel: Channel, timeout: Duration) -> Self {
        Self {
            client: AuthzServiceClient::new(channel),
            timeout,
        }
    }

    pub async fn connect(endpoint: impl AsRef<str>, timeout: Duration) -> Result<Self, PortError> {
        let channel = tonic::transport::Endpoint::from_shared(endpoint.as_ref().to_owned())
            .map_err(|_| PortError::InvalidInput)?
            .connect_timeout(timeout)
            .connect()
            .await
            .map_err(|_| PortError::Unavailable)?;
        Ok(Self::new(channel, timeout))
    }

    async fn call_with_deadline<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, tonic::Status>>,
    ) -> Result<T, PortError> {
        tokio::time::timeout(self.timeout, future)
            .await
            .map_err(|_| PortError::Unavailable)?
            .map_err(map_status)
    }
}

#[async_trait]
impl Authorization for AuthzGrpcAdapter {
    async fn check(&self, check: PermissionCheck) -> Result<AuthorizationDecision, PortError> {
        validate_check(&check)?;
        let mut request = Request::new(CheckPermissionRequest {
            check: Some(RpcPermissionCheck {
                resource: Some(ResourceReference {
                    resource_type: check.resource_type,
                    resource_id: check.resource_id,
                }),
                permission: check.permission,
                principal: Some(PrincipalReference {
                    principal_type: principal_type(check.principal.kind).to_owned(),
                    principal_id: check.principal.id,
                }),
            }),
            consistency_token: String::new(),
        });
        request.set_timeout(self.timeout);
        let mut client = self.client.clone();
        let response = self
            .call_with_deadline(async move { client.check_permission(request).await })
            .await?
            .into_inner();
        match PermissionDecision::try_from(response.decision) {
            Ok(PermissionDecision::Allowed) => Ok(AuthorizationDecision::Allowed),
            Ok(PermissionDecision::Denied) => Ok(AuthorizationDecision::Denied),
            Ok(PermissionDecision::Unspecified) | Err(_) => Err(PortError::Internal),
        }
    }

    async fn write_relationship(&self, write: RelationshipWrite) -> Result<(), PortError> {
        let operation = write.operation;
        if write.idempotency_key.trim().is_empty()
            || operation.resource_type.trim().is_empty()
            || operation.resource_id.trim().is_empty()
            || operation.relation.trim().is_empty()
            || operation.subject_type.trim().is_empty()
            || operation.subject_id.trim().is_empty()
        {
            return Err(PortError::InvalidInput);
        }
        let mut request = Request::new(WriteRelationshipsRequest {
            updates: vec![Relationship {
                resource: Some(ResourceReference {
                    resource_type: operation.resource_type,
                    resource_id: operation.resource_id,
                }),
                relation: operation.relation,
                subject: Some(PrincipalReference {
                    principal_type: operation.subject_type,
                    principal_id: operation.subject_id,
                }),
            }],
            idempotency_key: write.idempotency_key,
            deletions: Vec::new(),
        });
        request.set_timeout(self.timeout);
        let mut client = self.client.clone();
        let response = self
            .call_with_deadline(async move { client.write_relationships(request).await })
            .await?
            .into_inner();
        match response.outcome {
            Some(Outcome::Written(_)) => Ok(()),
            Some(Outcome::Rejection(_)) => Err(PortError::InvalidInput),
            None => Err(PortError::Internal),
        }
    }
}

fn validate_check(check: &PermissionCheck) -> Result<(), PortError> {
    if check.principal.id.trim().is_empty()
        || check.resource_type.trim().is_empty()
        || check.resource_id.trim().is_empty()
        || check.permission.trim().is_empty()
    {
        return Err(PortError::InvalidInput);
    }
    Ok(())
}

fn principal_type(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::User => "user",
        PrincipalKind::ServiceAccount => "service_account",
    }
}

fn map_status(status: tonic::Status) -> PortError {
    match status.code() {
        tonic::Code::InvalidArgument => PortError::InvalidInput,
        tonic::Code::DeadlineExceeded
        | tonic::Code::Unavailable
        | tonic::Code::Cancelled
        | tonic::Code::ResourceExhausted => PortError::Unavailable,
        _ => PortError::Internal,
    }
}
