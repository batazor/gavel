use async_trait::async_trait;
use gavel_authz_prost::gavel::authz::v1 as rpc;
use gavel_authz_tonic::gavel::authz::v1::tonic::authz_service_client::AuthzServiceClient;
use rpc::write_relationships_response;
use tonic_bsr::{Request, transport::Channel};

use crate::{
    application::{
        Authorization, AuthorizationDecision, AuthorizationRequest, Permission, PortError,
    },
    domain::PrincipalId,
};

#[derive(Clone)]
pub struct AuthzGrpc {
    client: AuthzServiceClient<Channel>,
}

impl AuthzGrpc {
    pub async fn connect(endpoint: impl AsRef<str>) -> Result<Self, PortError> {
        let channel = tonic_bsr::transport::Endpoint::from_shared(endpoint.as_ref().to_owned())
            .map_err(|_| PortError::Internal)?
            .connect()
            .await
            .map_err(|_| PortError::Unavailable)?;
        Ok(Self {
            client: AuthzServiceClient::new(channel),
        })
    }

    async fn check_rpc(
        &self,
        request: AuthorizationRequest,
    ) -> Result<AuthorizationDecision, PortError> {
        let (resource_type, resource_id, permission, principal_type, principal_id) =
            match request.permission {
                Permission::RegisterInstrument => (
                    "platform",
                    "gavel".to_owned(),
                    "register_payment_instrument",
                    "user",
                    request.principal,
                ),
                Permission::ManageInstrument => (
                    "payment_profile",
                    request.resource_id.ok_or(PortError::Internal)?,
                    "manage",
                    "user",
                    request.principal,
                ),
                Permission::ViewInstrument => (
                    "payment_profile",
                    request.resource_id.ok_or(PortError::Internal)?,
                    "view",
                    "user",
                    request.principal,
                ),
                Permission::ViewAttempt => (
                    "payment_attempt",
                    request.resource_id.ok_or(PortError::Internal)?,
                    "view",
                    "user",
                    request.principal,
                ),
                Permission::ContinuePayment => (
                    "payment_attempt",
                    request.resource_id.ok_or(PortError::Internal)?,
                    "continue_payment",
                    "user",
                    request.principal,
                ),
                Permission::ProcessPayment => (
                    "platform",
                    "gavel".to_owned(),
                    "operate_payments",
                    "service_account",
                    "processes".to_owned(),
                ),
            };
        let mut rpc_request = Request::new(rpc::CheckPermissionRequest {
            check: Some(rpc::PermissionCheck {
                resource: Some(rpc::ResourceReference {
                    resource_type: resource_type.to_owned(),
                    resource_id,
                }),
                permission: permission.to_owned(),
                principal: Some(rpc::PrincipalReference {
                    principal_type: principal_type.to_owned(),
                    principal_id,
                }),
            }),
            consistency_token: request.consistency_token.unwrap_or_default(),
        });
        rpc_request.set_timeout(std::time::Duration::from_millis(request.timeout_millis));
        let response = self
            .client
            .clone()
            .check_permission(rpc_request)
            .await
            .map_err(map_status)?
            .into_inner();
        Ok(AuthorizationDecision {
            allowed: response.decision == rpc::PermissionDecision::Allowed as i32,
            checked_at_token: response.checked_at_token,
        })
    }

    async fn write(
        &self,
        updates: Vec<rpc::Relationship>,
        deletions: Vec<rpc::Relationship>,
        key: &str,
        timeout_millis: u64,
    ) -> Result<String, PortError> {
        let mut request = Request::new(rpc::WriteRelationshipsRequest {
            updates,
            idempotency_key: key.to_owned(),
            deletions,
        });
        request.set_timeout(std::time::Duration::from_millis(timeout_millis));
        match self
            .client
            .clone()
            .write_relationships(request)
            .await
            .map_err(map_status)?
            .into_inner()
            .outcome
            .ok_or(PortError::Internal)?
        {
            write_relationships_response::Outcome::Written(value) => Ok(value.written_at_token),
            write_relationships_response::Outcome::Rejection(_) => Err(PortError::Conflict),
        }
    }
}

#[async_trait]
impl Authorization for AuthzGrpc {
    async fn check(
        &self,
        request: AuthorizationRequest,
    ) -> Result<AuthorizationDecision, PortError> {
        self.check_rpc(request).await
    }
    async fn touch_owner_grant(
        &self,
        instrument_id: &str,
        owner: &PrincipalId,
        idempotency_key: &str,
        timeout_millis: u64,
    ) -> Result<String, PortError> {
        self.write(
            vec![owner_relationship(instrument_id, owner)],
            vec![],
            idempotency_key,
            timeout_millis,
        )
        .await
    }
    async fn delete_owner_grant(
        &self,
        instrument_id: &str,
        owner: &PrincipalId,
        idempotency_key: &str,
        timeout_millis: u64,
    ) -> Result<(), PortError> {
        self.write(
            vec![],
            vec![owner_relationship(instrument_id, owner)],
            idempotency_key,
            timeout_millis,
        )
        .await
        .map(|_| ())
    }
}

fn owner_relationship(instrument_id: &str, owner: &PrincipalId) -> rpc::Relationship {
    rpc::Relationship {
        resource: Some(rpc::ResourceReference {
            resource_type: "payment_profile".to_owned(),
            resource_id: instrument_id.to_owned(),
        }),
        relation: "owner".to_owned(),
        subject: Some(rpc::PrincipalReference {
            principal_type: "user".to_owned(),
            principal_id: owner.to_string(),
        }),
    }
}

fn map_status(status: tonic_bsr::Status) -> PortError {
    match status.code() {
        tonic_bsr::Code::DeadlineExceeded => PortError::Timeout,
        tonic_bsr::Code::Unavailable
        | tonic_bsr::Code::Cancelled
        | tonic_bsr::Code::ResourceExhausted => PortError::Unavailable,
        tonic_bsr::Code::NotFound => PortError::NotFound,
        tonic_bsr::Code::AlreadyExists | tonic_bsr::Code::Aborted => PortError::Conflict,
        _ => PortError::Internal,
    }
}
