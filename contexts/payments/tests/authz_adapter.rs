use gavel_authz_prost::gavel::authz::v1 as rpc;
use gavel_authz_tonic::gavel::authz::v1::tonic::authz_service_server::{
    AuthzService, AuthzServiceServer,
};
use gavel_payments::{
    adapters::outbound::AuthzGrpc,
    application::{Authorization, AuthorizationRequest, Permission},
    domain::PrincipalId,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio_stream::wrappers::TcpListenerStream;

struct FakeAuthz {
    allowed: AtomicBool,
    checks: Mutex<Vec<rpc::CheckPermissionRequest>>,
    writes: Mutex<Vec<rpc::WriteRelationshipsRequest>>,
}

#[tonic_bsr::async_trait]
impl AuthzService for FakeAuthz {
    async fn check_permission(
        &self,
        request: tonic_bsr::Request<rpc::CheckPermissionRequest>,
    ) -> Result<tonic_bsr::Response<rpc::CheckPermissionResponse>, tonic_bsr::Status> {
        self.checks.lock().unwrap().push(request.into_inner());
        Ok(tonic_bsr::Response::new(rpc::CheckPermissionResponse {
            decision: if self.allowed.load(Ordering::SeqCst) {
                rpc::PermissionDecision::Allowed as i32
            } else {
                rpc::PermissionDecision::Denied as i32
            },
            checked_at_token: "revision-1".to_owned(),
        }))
    }
    async fn bulk_check_permissions(
        &self,
        _: tonic_bsr::Request<rpc::BulkCheckPermissionsRequest>,
    ) -> Result<tonic_bsr::Response<rpc::BulkCheckPermissionsResponse>, tonic_bsr::Status> {
        Err(tonic_bsr::Status::unimplemented("unused"))
    }
    async fn register_principal(
        &self,
        _: tonic_bsr::Request<rpc::RegisterPrincipalRequest>,
    ) -> Result<tonic_bsr::Response<rpc::RegisterPrincipalResponse>, tonic_bsr::Status> {
        Err(tonic_bsr::Status::unimplemented("unused"))
    }
    async fn write_relationships(
        &self,
        request: tonic_bsr::Request<rpc::WriteRelationshipsRequest>,
    ) -> Result<tonic_bsr::Response<rpc::WriteRelationshipsResponse>, tonic_bsr::Status> {
        self.writes.lock().unwrap().push(request.into_inner());
        Ok(tonic_bsr::Response::new(rpc::WriteRelationshipsResponse {
            outcome: Some(rpc::write_relationships_response::Outcome::Written(
                rpc::RelationshipsWritten {
                    written_at_token: "revision-2".to_owned(),
                },
            )),
        }))
    }
}

#[tokio::test]
async fn maps_every_payment_permission_and_owner_grant_with_causal_tokens() {
    let fake = Arc::new(FakeAuthz {
        allowed: AtomicBool::new(true),
        checks: Mutex::new(vec![]),
        writes: Mutex::new(vec![]),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_fake = fake.clone();
    tokio::spawn(async move {
        tonic_bsr::transport::Server::builder()
            .add_service(AuthzServiceServer::from_arc(server_fake))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let adapter = AuthzGrpc::connect(format!("http://{address}"))
        .await
        .unwrap();
    let cases = [
        (Permission::RegisterInstrument, None),
        (Permission::ManageInstrument, Some("instrument-1")),
        (Permission::ViewInstrument, Some("instrument-1")),
        (Permission::ViewAttempt, Some("attempt-1")),
        (Permission::ContinuePayment, Some("attempt-1")),
        (Permission::ProcessPayment, Some("auction-1")),
    ];
    for (permission, resource) in cases {
        let decision = adapter
            .check(AuthorizationRequest {
                principal: "user-1".to_owned(),
                permission,
                resource_id: resource.map(str::to_owned),
                consistency_token: Some("revision-0".to_owned()),
                timeout_millis: 1_000,
            })
            .await
            .unwrap();
        assert!(decision.allowed);
        assert_eq!(decision.checked_at_token, "revision-1");
    }
    let owner = PrincipalId::parse("user-1").unwrap();
    assert_eq!(
        adapter
            .touch_owner_grant("instrument-1", &owner, "grant-1", 1_000)
            .await
            .unwrap(),
        "revision-2"
    );
    adapter
        .delete_owner_grant("instrument-1", &owner, "grant-2", 1_000)
        .await
        .unwrap();
    fake.allowed.store(false, Ordering::SeqCst);
    assert!(
        !adapter
            .check(AuthorizationRequest {
                principal: "user-1".to_owned(),
                permission: Permission::ViewInstrument,
                resource_id: Some("instrument-1".to_owned()),
                consistency_token: None,
                timeout_millis: 1_000
            })
            .await
            .unwrap()
            .allowed
    );

    let checks = fake.checks.lock().unwrap();
    assert_eq!(checks.len(), 7);
    assert!(
        checks
            .iter()
            .all(|request| request.consistency_token == "revision-0"
                || request.consistency_token.is_empty())
    );
    assert_eq!(
        checks[5]
            .check
            .as_ref()
            .unwrap()
            .principal
            .as_ref()
            .unwrap()
            .principal_type,
        "service_account"
    );
    let writes = fake.writes.lock().unwrap();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].updates[0].relation, "owner");
    assert_eq!(writes[1].deletions[0].relation, "owner");
}
