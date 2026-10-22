use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicU64, Ordering},
    },
    time::Duration,
};

use gavel_authz::{
    adapters::outbound::SpiceDbAdapter,
    application::{
        PermissionCheck, PolicyDecision, PolicyEngine, PolicyEngineError, PrincipalReference,
        ResourceReference,
    },
    spicedb_rpc::{
        self as rpc,
        permissions_service_server::{PermissionsService, PermissionsServiceServer},
    },
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status, transport::Server};

#[derive(Clone)]
struct MockSpiceDb {
    single_decision: Arc<AtomicI32>,
    bulk_decisions: Arc<Mutex<Vec<i32>>>,
    delay_millis: Arc<AtomicU64>,
    authorization: Arc<Mutex<Vec<String>>>,
    single_requests: Arc<Mutex<Vec<rpc::CheckPermissionRequest>>>,
    write_requests: Arc<Mutex<Vec<rpc::WriteRelationshipsRequest>>>,
}

impl Default for MockSpiceDb {
    fn default() -> Self {
        Self {
            single_decision: Arc::new(AtomicI32::new(
                rpc::check_permission_response::Permissionship::HasPermission as i32,
            )),
            bulk_decisions: Arc::new(Mutex::new(vec![])),
            delay_millis: Arc::new(AtomicU64::new(0)),
            authorization: Arc::new(Mutex::new(vec![])),
            single_requests: Arc::new(Mutex::new(vec![])),
            write_requests: Arc::new(Mutex::new(vec![])),
        }
    }
}

impl MockSpiceDb {
    fn capture_authorization<T>(&self, request: &Request<T>) {
        self.authorization.lock().unwrap().push(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        );
    }
}

#[tonic::async_trait]
impl PermissionsService for MockSpiceDb {
    async fn write_relationships(
        &self,
        request: Request<rpc::WriteRelationshipsRequest>,
    ) -> Result<Response<rpc::WriteRelationshipsResponse>, Status> {
        self.capture_authorization(&request);
        self.write_requests
            .lock()
            .unwrap()
            .push(request.into_inner());
        Ok(Response::new(rpc::WriteRelationshipsResponse {
            written_at: Some(rpc::ZedToken {
                token: "write-revision".to_owned(),
            }),
        }))
    }

    async fn check_permission(
        &self,
        request: Request<rpc::CheckPermissionRequest>,
    ) -> Result<Response<rpc::CheckPermissionResponse>, Status> {
        self.capture_authorization(&request);
        self.single_requests
            .lock()
            .unwrap()
            .push(request.into_inner());
        tokio::time::sleep(Duration::from_millis(
            self.delay_millis.load(Ordering::SeqCst),
        ))
        .await;
        Ok(Response::new(rpc::CheckPermissionResponse {
            checked_at: Some(rpc::ZedToken {
                token: "single-revision".to_owned(),
            }),
            permissionship: self.single_decision.load(Ordering::SeqCst),
        }))
    }

    async fn check_bulk_permissions(
        &self,
        request: Request<rpc::CheckBulkPermissionsRequest>,
    ) -> Result<Response<rpc::CheckBulkPermissionsResponse>, Status> {
        self.capture_authorization(&request);
        let request = request.into_inner();
        let decisions = self.bulk_decisions.lock().unwrap().clone();
        let pairs = request
            .items
            .into_iter()
            .zip(decisions)
            .map(|(item, permissionship)| rpc::CheckBulkPermissionsPair {
                request: Some(item),
                response: Some(rpc::check_bulk_permissions_pair::Response::Item(
                    rpc::CheckBulkPermissionsResponseItem { permissionship },
                )),
            })
            .collect();
        Ok(Response::new(rpc::CheckBulkPermissionsResponse {
            checked_at: Some(rpc::ZedToken {
                token: "bulk-revision".to_owned(),
            }),
            pairs,
        }))
    }
}

async fn adapter(mock: MockSpiceDb, timeout: Duration) -> SpiceDbAdapter {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(PermissionsServiceServer::new(mock))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    SpiceDbAdapter::connect(&format!("http://{address}"), "test-secret", timeout)
        .await
        .unwrap()
}

fn check(id: &str, permission: &str) -> PermissionCheck {
    PermissionCheck {
        resource: ResourceReference {
            resource_type: "auction".to_owned(),
            resource_id: id.to_owned(),
        },
        permission: permission.to_owned(),
        principal: PrincipalReference {
            principal_type: "user".to_owned(),
            principal_id: "u1_viewer".to_owned(),
        },
    }
}

#[tokio::test]
async fn single_check_maps_allow_deny_token_consistency_and_credential() {
    let mock = MockSpiceDb::default();
    let client = adapter(mock.clone(), Duration::from_secs(1)).await;
    let allowed = client
        .check_permission(&check("auction-1", "place_bid"), Some("minimum-revision"))
        .await
        .unwrap();
    assert_eq!(allowed.decision, PolicyDecision::Allowed);
    assert_eq!(allowed.checked_at_token, "single-revision");
    mock.single_decision.store(
        rpc::check_permission_response::Permissionship::NoPermission as i32,
        Ordering::SeqCst,
    );
    assert_eq!(
        client
            .check_permission(&check("auction-1", "place_bid"), None)
            .await
            .unwrap()
            .decision,
        PolicyDecision::Denied
    );
    let requests = mock.single_requests.lock().unwrap();
    assert!(matches!(
        requests[0]
            .consistency
            .as_ref()
            .and_then(|value| value.requirement.as_ref()),
        Some(rpc::consistency::Requirement::AtLeastAsFresh(token)) if token.token == "minimum-revision"
    ));
    assert_eq!(
        &*mock.authorization.lock().unwrap(),
        &["Bearer test-secret", "Bearer test-secret"]
    );
}

#[tokio::test]
async fn bulk_check_preserves_order_and_common_token() {
    let mock = MockSpiceDb::default();
    *mock.bulk_decisions.lock().unwrap() = vec![
        rpc::check_permission_response::Permissionship::NoPermission as i32,
        rpc::check_permission_response::Permissionship::HasPermission as i32,
        rpc::check_permission_response::Permissionship::NoPermission as i32,
    ];
    let client = adapter(mock, Duration::from_secs(1)).await;
    let result = client
        .bulk_check_permissions(
            &[
                check("auction-1", "place_bid"),
                check("auction-2", "view_personal_state"),
                check("auction-3", "reschedule"),
            ],
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        result.decisions,
        vec![
            PolicyDecision::Denied,
            PolicyDecision::Allowed,
            PolicyDecision::Denied
        ]
    );
    assert_eq!(result.checked_at_token, "bulk-revision");
}

#[tokio::test]
async fn timeout_and_unknown_decision_fail_closed() {
    let timeout_mock = MockSpiceDb::default();
    timeout_mock.delay_millis.store(100, Ordering::SeqCst);
    let client = adapter(timeout_mock, Duration::from_millis(10)).await;
    assert_eq!(
        client
            .check_permission(&check("auction-1", "place_bid"), None)
            .await,
        Err(PolicyEngineError::Timeout)
    );

    let unknown_mock = MockSpiceDb::default();
    unknown_mock.single_decision.store(99, Ordering::SeqCst);
    let client = adapter(unknown_mock, Duration::from_secs(1)).await;
    assert_eq!(
        client
            .check_permission(&check("auction-1", "place_bid"), None)
            .await,
        Err(PolicyEngineError::InvalidResponse)
    );
}

#[tokio::test]
async fn relationship_batch_is_one_atomic_touch_delete_call_and_readiness_is_live() {
    use gavel_authz::application::{PolicyWriteResult, Relationship};

    let mock = MockSpiceDb::default();
    let client = adapter(mock.clone(), Duration::from_secs(1)).await;
    client.ready().await.unwrap();
    let relationship = Relationship {
        resource: ResourceReference {
            resource_type: "auction".to_owned(),
            resource_id: "auction-1".to_owned(),
        },
        relation: "participant".to_owned(),
        subject: PrincipalReference {
            principal_type: "user".to_owned(),
            principal_id: "u1_bidder".to_owned(),
        },
    };
    let expected = PolicyWriteResult {
        written_at_token: "write-revision".to_owned(),
    };
    assert_eq!(
        client
            .write_relationships(
                std::slice::from_ref(&relationship),
                std::slice::from_ref(&relationship),
            )
            .await,
        Ok(expected.clone())
    );
    assert_eq!(
        client
            .write_relationships(std::slice::from_ref(&relationship), &[])
            .await,
        Ok(expected)
    );

    let requests = mock.write_requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].updates.len(), 2);
    assert_eq!(
        requests[0].updates[0].operation,
        rpc::relationship_update::Operation::Touch as i32
    );
    assert_eq!(
        requests[0].updates[1].operation,
        rpc::relationship_update::Operation::Delete as i32
    );
    assert_eq!(
        requests[1].updates[0].operation,
        rpc::relationship_update::Operation::Touch as i32
    );
    assert_eq!(mock.authorization.lock().unwrap().len(), 2);
}
