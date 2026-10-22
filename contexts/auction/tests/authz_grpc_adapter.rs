use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use gavel_auction::{
    adapters::outbound::authz::AuthzGrpcAdapter,
    application::{
        Authorization, AuthorizationDecision, PermissionCheck, PortError, Principal, PrincipalKind,
        ReconciliationOperation, RelationshipWrite,
    },
    authz_rpc::{
        BulkCheckPermissionsRequest, BulkCheckPermissionsResponse, CheckPermissionRequest,
        CheckPermissionResponse, PermissionDecision, RegisterPrincipalRequest,
        RegisterPrincipalResponse, RelationshipsWritten, WriteRelationshipsRequest,
        WriteRelationshipsResponse,
        authz_service_server::{AuthzService, AuthzServiceServer},
        write_relationships_response::Outcome,
    },
};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status, transport::Server};

#[derive(Clone, Copy)]
enum CheckBehavior {
    Allowed,
    Denied,
    Unavailable,
    Delayed,
}

#[derive(Clone)]
struct MockAuthz {
    behavior: CheckBehavior,
    write_keys: Arc<Mutex<Vec<String>>>,
}

#[tonic::async_trait]
impl AuthzService for MockAuthz {
    async fn check_permission(
        &self,
        _request: Request<CheckPermissionRequest>,
    ) -> Result<Response<CheckPermissionResponse>, Status> {
        let decision = match self.behavior {
            CheckBehavior::Allowed => PermissionDecision::Allowed,
            CheckBehavior::Denied => PermissionDecision::Denied,
            CheckBehavior::Unavailable => return Err(Status::unavailable("mock unavailable")),
            CheckBehavior::Delayed => {
                tokio::time::sleep(Duration::from_millis(200)).await;
                PermissionDecision::Allowed
            }
        };
        Ok(Response::new(CheckPermissionResponse {
            decision: decision.into(),
            checked_at_token: "token-1".to_owned(),
        }))
    }

    async fn bulk_check_permissions(
        &self,
        _request: Request<BulkCheckPermissionsRequest>,
    ) -> Result<Response<BulkCheckPermissionsResponse>, Status> {
        Err(Status::unimplemented("not used by Auction"))
    }

    async fn write_relationships(
        &self,
        request: Request<WriteRelationshipsRequest>,
    ) -> Result<Response<WriteRelationshipsResponse>, Status> {
        self.write_keys
            .lock()
            .unwrap()
            .push(request.into_inner().idempotency_key);
        Ok(Response::new(WriteRelationshipsResponse {
            outcome: Some(Outcome::Written(RelationshipsWritten {
                written_at_token: "token-2".to_owned(),
            })),
        }))
    }

    async fn register_principal(
        &self,
        _request: Request<RegisterPrincipalRequest>,
    ) -> Result<Response<RegisterPrincipalResponse>, Status> {
        Err(Status::unimplemented("not used by Auction"))
    }
}

async fn adapter(
    behavior: CheckBehavior,
    timeout: Duration,
) -> (
    AuthzGrpcAdapter,
    Arc<Mutex<Vec<String>>>,
    oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let write_keys = Arc::new(Mutex::new(Vec::new()));
    let service = MockAuthz {
        behavior,
        write_keys: Arc::clone(&write_keys),
    };
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        Server::builder()
            .add_service(AuthzServiceServer::new(service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    let adapter = AuthzGrpcAdapter::connect(format!("http://{address}"), timeout)
        .await
        .unwrap();
    (adapter, write_keys, shutdown_tx)
}

fn check() -> PermissionCheck {
    PermissionCheck {
        principal: Principal {
            kind: PrincipalKind::User,
            id: "user-1".to_owned(),
        },
        resource_type: "auction".to_owned(),
        resource_id: "auction-1".to_owned(),
        permission: "place_bid".to_owned(),
    }
}

#[tokio::test]
async fn maps_allowed_and_denied_decisions() {
    let (allowed, _, allowed_shutdown) =
        adapter(CheckBehavior::Allowed, Duration::from_secs(1)).await;
    assert_eq!(
        allowed.check(check()).await.unwrap(),
        AuthorizationDecision::Allowed
    );
    let _ = allowed_shutdown.send(());

    let (denied, _, denied_shutdown) = adapter(CheckBehavior::Denied, Duration::from_secs(1)).await;
    assert_eq!(
        denied.check(check()).await.unwrap(),
        AuthorizationDecision::Denied
    );
    let _ = denied_shutdown.send(());
}

#[tokio::test]
async fn deadline_and_transport_failures_fail_closed_as_unavailable() {
    let (delayed, _, delayed_shutdown) =
        adapter(CheckBehavior::Delayed, Duration::from_millis(30)).await;
    assert_eq!(delayed.check(check()).await, Err(PortError::Unavailable));
    let _ = delayed_shutdown.send(());

    let (unavailable, _, unavailable_shutdown) =
        adapter(CheckBehavior::Unavailable, Duration::from_secs(1)).await;
    assert_eq!(
        unavailable.check(check()).await,
        Err(PortError::Unavailable)
    );
    let _ = unavailable_shutdown.send(());
}

#[tokio::test]
async fn relationship_writes_preserve_the_idempotency_key() {
    let (adapter, keys, shutdown) = adapter(CheckBehavior::Allowed, Duration::from_secs(1)).await;
    let write = RelationshipWrite {
        idempotency_key: "relationship-operation-1".to_owned(),
        operation: ReconciliationOperation {
            operation_id: "relationship-operation-1".to_owned(),
            resource_type: "auction".to_owned(),
            resource_id: "auction-1".to_owned(),
            relation: "participant".to_owned(),
            subject_type: "user".to_owned(),
            subject_id: "user-1".to_owned(),
        },
    };
    adapter.write_relationship(write.clone()).await.unwrap();
    adapter.write_relationship(write).await.unwrap();
    assert_eq!(
        *keys.lock().unwrap(),
        vec![
            "relationship-operation-1".to_owned(),
            "relationship-operation-1".to_owned()
        ]
    );
    let _ = shutdown.send(());
}
