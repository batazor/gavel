use std::time::Duration;

use async_trait::async_trait;
use gavel_authz::rpc::{
    CheckPermissionRequest, PermissionCheck as RpcPermissionCheck,
    PermissionDecision as RpcDecision, PrincipalReference, Relationship, ResourceReference,
    WriteRelationshipsRequest, authz_service_client::AuthzServiceClient,
    write_relationships_response::Outcome,
};
use tonic::{Request, transport::Channel};
use tower::ServiceExt;

use crate::{
    application::{
        Authorization, AuthorizationRevision, DeleteLotOwnerGrant, LotOwnerGrant, Permission,
        PermissionCheck, PermissionDecision, PortError, ServicePermissionCheck, WorkloadIdentity,
    },
    domain::PrincipalId,
};

#[derive(Clone)]
pub struct AuthzGrpcAdapter {
    client: AuthzServiceClient<Channel>,
    channel: Channel,
}

struct PermissionRpcRequest {
    resource_type: &'static str,
    resource_id: String,
    permission: &'static str,
    principal_type: &'static str,
    principal_id: String,
    consistency_token: Option<String>,
    timeout_millis: u64,
}

impl AuthzGrpcAdapter {
    pub fn new(channel: Channel) -> Self {
        Self {
            client: AuthzServiceClient::new(channel.clone()),
            channel,
        }
    }

    pub async fn connect(
        endpoint: impl AsRef<str>,
        connect_timeout: Duration,
    ) -> Result<Self, PortError> {
        if connect_timeout.is_zero() {
            return Err(PortError::Internal);
        }
        let channel = tonic::transport::Endpoint::from_shared(endpoint.as_ref().to_owned())
            .map_err(|_| PortError::Internal)?
            .connect_timeout(connect_timeout)
            .connect()
            .await
            .map_err(|_| PortError::Unavailable)?;
        Ok(Self::new(channel))
    }

    pub async fn ready(&self) -> Result<(), PortError> {
        self.channel
            .clone()
            .ready()
            .await
            .map(|_| ())
            .map_err(|_| PortError::Unavailable)
    }

    async fn permission(
        &self,
        input: PermissionRpcRequest,
    ) -> Result<PermissionDecision, PortError> {
        let timeout = timeout(input.timeout_millis)?;
        validate(&input.resource_id)?;
        validate(&input.principal_id)?;
        if input
            .consistency_token
            .as_ref()
            .is_some_and(|value| value.is_empty())
        {
            return Err(PortError::Internal);
        }
        let mut request = Request::new(CheckPermissionRequest {
            check: Some(RpcPermissionCheck {
                resource: Some(ResourceReference {
                    resource_type: input.resource_type.to_owned(),
                    resource_id: input.resource_id,
                }),
                permission: input.permission.to_owned(),
                principal: Some(PrincipalReference {
                    principal_type: input.principal_type.to_owned(),
                    principal_id: input.principal_id,
                }),
            }),
            consistency_token: input.consistency_token.unwrap_or_default(),
        });
        request.set_timeout(timeout);
        let mut client = self.client.clone();
        let response = tokio::time::timeout(timeout, client.check_permission(request))
            .await
            .map_err(|_| PortError::Timeout)?
            .map_err(map_status)?
            .into_inner();
        let allowed = match RpcDecision::try_from(response.decision) {
            Ok(RpcDecision::Allowed) => true,
            Ok(RpcDecision::Denied) => false,
            Ok(RpcDecision::Unspecified) | Err(_) => return Err(PortError::Internal),
        };
        if response.checked_at_token.is_empty() {
            return Err(PortError::Internal);
        }
        Ok(PermissionDecision {
            allowed,
            checked_at_token: response.checked_at_token,
        })
    }

    async fn write(
        &self,
        updates: Vec<Relationship>,
        deletions: Vec<Relationship>,
        idempotency_key: String,
        timeout_millis: u64,
    ) -> Result<AuthorizationRevision, PortError> {
        validate(&idempotency_key)?;
        let timeout = timeout(timeout_millis)?;
        let mut request = Request::new(WriteRelationshipsRequest {
            updates,
            idempotency_key,
            deletions,
        });
        request.set_timeout(timeout);
        let mut client = self.client.clone();
        let response = tokio::time::timeout(timeout, client.write_relationships(request))
            .await
            .map_err(|_| PortError::Timeout)?
            .map_err(map_status)?
            .into_inner();
        match response.outcome {
            Some(Outcome::Written(written)) if !written.written_at_token.is_empty() => {
                Ok(AuthorizationRevision {
                    revision: written.written_at_token,
                })
            }
            Some(Outcome::Written(_)) | Some(Outcome::Rejection(_)) | None => {
                Err(PortError::Internal)
            }
        }
    }
}

#[async_trait]
impl Authorization for AuthzGrpcAdapter {
    async fn check(&self, request: PermissionCheck) -> Result<PermissionDecision, PortError> {
        let (resource_type, resource_id, permission) = match request.permission {
            Permission::CreateLot => ("platform", "gavel".to_owned(), "create_lot"),
            Permission::EditLot => (
                "lot",
                request.resource_id.ok_or(PortError::Internal)?,
                "edit",
            ),
            Permission::ExecuteWithdraw => return Err(PortError::Internal),
        };
        self.permission(PermissionRpcRequest {
            resource_type,
            resource_id,
            permission,
            principal_type: "user",
            principal_id: request.principal.to_string(),
            consistency_token: request.consistency_token,
            timeout_millis: request.timeout_millis,
        })
        .await
    }

    async fn check_service(
        &self,
        request: ServicePermissionCheck,
    ) -> Result<PermissionDecision, PortError> {
        if request.workload != WorkloadIdentity::Processes
            || request.permission != Permission::ExecuteWithdraw
        {
            return Err(PortError::Internal);
        }
        self.permission(PermissionRpcRequest {
            resource_type: "lot",
            resource_id: request.resource_id,
            permission: "execute_withdraw",
            principal_type: "service_account",
            principal_id: "processes".to_owned(),
            consistency_token: request.consistency_token,
            timeout_millis: request.timeout_millis,
        })
        .await
    }

    async fn touch_lot_owner(
        &self,
        request: LotOwnerGrant,
    ) -> Result<AuthorizationRevision, PortError> {
        validate(&request.lot_id)?;
        validate_principal(&request.seller)?;
        if request
            .consistency_token
            .as_ref()
            .is_some_and(|value| value.is_empty())
        {
            return Err(PortError::Internal);
        }
        self.write(
            lot_relationships(&request.lot_id, &request.seller),
            Vec::new(),
            request.idempotency_key,
            request.timeout_millis,
        )
        .await
    }

    async fn delete_lot_owner(
        &self,
        request: DeleteLotOwnerGrant,
    ) -> Result<AuthorizationRevision, PortError> {
        validate(&request.lot_id)?;
        validate_principal(&request.seller)?;
        self.write(
            Vec::new(),
            lot_relationships(&request.lot_id, &request.seller),
            request.idempotency_key,
            request.timeout_millis,
        )
        .await
    }
}

fn lot_relationships(lot_id: &str, seller: &PrincipalId) -> Vec<Relationship> {
    vec![
        Relationship {
            resource: Some(ResourceReference {
                resource_type: "lot".to_owned(),
                resource_id: lot_id.to_owned(),
            }),
            relation: "platform".to_owned(),
            subject: Some(PrincipalReference {
                principal_type: "platform".to_owned(),
                principal_id: "gavel".to_owned(),
            }),
        },
        Relationship {
            resource: Some(ResourceReference {
                resource_type: "lot".to_owned(),
                resource_id: lot_id.to_owned(),
            }),
            relation: "seller".to_owned(),
            subject: Some(PrincipalReference {
                principal_type: "user".to_owned(),
                principal_id: seller.to_string(),
            }),
        },
    ]
}

fn validate(value: &str) -> Result<(), PortError> {
    if value.trim().is_empty() || value.len() > 256 {
        Err(PortError::Internal)
    } else {
        Ok(())
    }
}

fn validate_principal(value: &PrincipalId) -> Result<(), PortError> {
    validate(&value.to_string())
}

fn timeout(millis: u64) -> Result<Duration, PortError> {
    if millis == 0 {
        Err(PortError::Internal)
    } else {
        Ok(Duration::from_millis(millis))
    }
}

fn map_status(status: tonic::Status) -> PortError {
    match status.code() {
        tonic::Code::DeadlineExceeded => PortError::Timeout,
        tonic::Code::Unavailable | tonic::Code::Cancelled | tonic::Code::ResourceExhausted => {
            PortError::Unavailable
        }
        tonic::Code::NotFound => PortError::NotFound,
        _ => PortError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gavel_authz::rpc::{
        BulkCheckPermissionsRequest, BulkCheckPermissionsResponse, CheckPermissionResponse,
        RegisterPrincipalRequest, RegisterPrincipalResponse, RelationshipsWritten,
        WriteRelationshipsResponse,
        authz_service_server::{AuthzService, AuthzServiceServer},
    };
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Response, Status, transport::Server};

    use super::*;

    #[derive(Default)]
    struct Calls {
        checks: Vec<CheckPermissionRequest>,
        writes: Vec<WriteRelationshipsRequest>,
        deadline_count: usize,
    }

    struct FakeAuthz {
        calls: Arc<Mutex<Calls>>,
        decision: RpcDecision,
        failure: bool,
    }

    #[tonic::async_trait]
    impl AuthzService for FakeAuthz {
        async fn check_permission(
            &self,
            request: Request<CheckPermissionRequest>,
        ) -> Result<Response<CheckPermissionResponse>, Status> {
            if self.failure {
                return Err(Status::unavailable("private backend failure"));
            }
            let mut calls = self.calls.lock().unwrap();
            calls.deadline_count += usize::from(request.metadata().contains_key("grpc-timeout"));
            calls.checks.push(request.into_inner());
            Ok(Response::new(CheckPermissionResponse {
                decision: self.decision as i32,
                checked_at_token: "revision-7".to_owned(),
            }))
        }

        async fn bulk_check_permissions(
            &self,
            _request: Request<BulkCheckPermissionsRequest>,
        ) -> Result<Response<BulkCheckPermissionsResponse>, Status> {
            Err(Status::unimplemented("unused"))
        }

        async fn register_principal(
            &self,
            _request: Request<RegisterPrincipalRequest>,
        ) -> Result<Response<RegisterPrincipalResponse>, Status> {
            Err(Status::unimplemented("unused"))
        }

        async fn write_relationships(
            &self,
            request: Request<WriteRelationshipsRequest>,
        ) -> Result<Response<WriteRelationshipsResponse>, Status> {
            if self.failure {
                return Err(Status::unavailable("private backend failure"));
            }
            let mut calls = self.calls.lock().unwrap();
            calls.deadline_count += usize::from(request.metadata().contains_key("grpc-timeout"));
            calls.writes.push(request.into_inner());
            Ok(Response::new(WriteRelationshipsResponse {
                outcome: Some(Outcome::Written(RelationshipsWritten {
                    written_at_token: "revision-8".to_owned(),
                })),
            }))
        }
    }

    async fn adapter(
        decision: RpcDecision,
        failure: bool,
    ) -> (
        AuthzGrpcAdapter,
        Arc<Mutex<Calls>>,
        tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
    ) {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(
            Server::builder()
                .add_service(AuthzServiceServer::new(FakeAuthz {
                    calls: Arc::clone(&calls),
                    decision,
                    failure,
                }))
                .serve_with_incoming(TcpListenerStream::new(listener)),
        );
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        (AuthzGrpcAdapter::new(channel), calls, server)
    }

    fn seller() -> PrincipalId {
        PrincipalId::new("seller-1".to_owned()).unwrap()
    }

    #[tokio::test]
    async fn maps_user_and_service_checks_with_causal_token_and_deadline() {
        let (adapter, calls, server) = adapter(RpcDecision::Allowed, false).await;
        let create = adapter
            .check(PermissionCheck {
                principal: seller(),
                permission: Permission::CreateLot,
                resource_id: None,
                consistency_token: Some("causal-1".to_owned()),
                timeout_millis: 500,
            })
            .await
            .unwrap();
        assert!(create.allowed);
        adapter
            .check(PermissionCheck {
                principal: seller(),
                permission: Permission::EditLot,
                resource_id: Some("lot-1".to_owned()),
                consistency_token: Some("causal-2".to_owned()),
                timeout_millis: 500,
            })
            .await
            .unwrap();
        adapter
            .check_service(ServicePermissionCheck {
                workload: WorkloadIdentity::Processes,
                permission: Permission::ExecuteWithdraw,
                resource_id: "lot-1".to_owned(),
                consistency_token: Some("causal-3".to_owned()),
                timeout_millis: 500,
            })
            .await
            .unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.deadline_count, 3);
        assert_eq!(calls.checks[0].consistency_token, "causal-1");
        let checks: Vec<_> = calls
            .checks
            .iter()
            .map(|request| {
                let check = request.check.as_ref().unwrap();
                let resource = check.resource.as_ref().unwrap();
                let principal = check.principal.as_ref().unwrap();
                (
                    resource.resource_type.as_str(),
                    resource.resource_id.as_str(),
                    check.permission.as_str(),
                    principal.principal_type.as_str(),
                    principal.principal_id.as_str(),
                )
            })
            .collect();
        assert_eq!(
            checks,
            vec![
                ("platform", "gavel", "create_lot", "user", "seller-1"),
                ("lot", "lot-1", "edit", "user", "seller-1"),
                (
                    "lot",
                    "lot-1",
                    "execute_withdraw",
                    "service_account",
                    "processes"
                ),
            ]
        );
        drop(calls);
        server.abort();
    }

    #[tokio::test]
    async fn writes_atomic_platform_then_seller_touch_and_delete_batches() {
        let (adapter, calls, server) = adapter(RpcDecision::Allowed, false).await;
        let revision = adapter
            .touch_lot_owner(LotOwnerGrant {
                lot_id: "lot-1".to_owned(),
                seller: seller(),
                idempotency_key: "touch-1".to_owned(),
                consistency_token: Some("revision-7".to_owned()),
                timeout_millis: 500,
            })
            .await
            .unwrap();
        assert_eq!(revision.revision, "revision-8");
        adapter
            .delete_lot_owner(DeleteLotOwnerGrant {
                lot_id: "lot-1".to_owned(),
                seller: seller(),
                idempotency_key: "delete-1".to_owned(),
                timeout_millis: 500,
            })
            .await
            .unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.writes.len(), 2);
        assert_eq!(calls.writes[0].updates.len(), 2);
        assert_eq!(calls.writes[0].updates[0].relation, "platform");
        assert_eq!(calls.writes[0].updates[1].relation, "seller");
        assert!(calls.writes[0].deletions.is_empty());
        assert!(calls.writes[1].updates.is_empty());
        assert_eq!(calls.writes[1].deletions[0].relation, "platform");
        assert_eq!(calls.writes[1].deletions[1].relation, "seller");
        drop(calls);
        server.abort();
    }

    #[tokio::test]
    async fn deny_is_a_decision_and_transport_failures_are_fail_closed() {
        let (denied, _, denied_server) = adapter(RpcDecision::Denied, false).await;
        let decision = denied
            .check(PermissionCheck {
                principal: seller(),
                permission: Permission::CreateLot,
                resource_id: None,
                consistency_token: None,
                timeout_millis: 500,
            })
            .await
            .unwrap();
        assert!(!decision.allowed);
        denied_server.abort();

        let (failing, _, failing_server) = adapter(RpcDecision::Allowed, true).await;
        assert_eq!(
            failing
                .check(PermissionCheck {
                    principal: seller(),
                    permission: Permission::CreateLot,
                    resource_id: None,
                    consistency_token: None,
                    timeout_millis: 500,
                })
                .await,
            Err(PortError::Unavailable)
        );
        assert_eq!(
            failing
                .check_service(ServicePermissionCheck {
                    workload: WorkloadIdentity::Untrusted,
                    permission: Permission::ExecuteWithdraw,
                    resource_id: "lot-1".to_owned(),
                    consistency_token: None,
                    timeout_millis: 500,
                })
                .await,
            Err(PortError::Internal)
        );
        failing_server.abort();
    }
}
