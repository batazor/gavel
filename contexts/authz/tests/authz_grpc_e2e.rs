use std::time::Duration;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use gavel_auction::{
    adapters::outbound::authz::AuthzGrpcAdapter,
    application::{
        Authorization, AuthorizationDecision, PermissionCheck as AuctionPermissionCheck, Principal,
        PrincipalKind, ReconciliationOperation, RelationshipWrite,
    },
};
use gavel_authz::{
    adapters::inbound::{AuthzGrpcService, WorkloadIdentityInterceptor},
    application::{
        CommandReceiptStore, MutationResult, PermissionCheck, PolicyBulkCheckResult,
        PolicyCheckResult, PolicyDecision, PolicyEngine, PolicyEngineError, PolicyWriteResult,
        ReceiptClaim, ReceiptClaimRequest, ReceiptLease, ReceiptStoreError, Relationship,
        WorkloadIdentity,
    },
    rpc::{
        self, authz_service_client::AuthzServiceClient, authz_service_server::AuthzServiceServer,
    },
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, transport::Server};

struct FakePolicy {
    decision: AtomicI32,
    writes: AtomicUsize,
    batches: Mutex<Vec<(Vec<Relationship>, Vec<Relationship>)>>,
}

#[async_trait]
impl PolicyEngine for FakePolicy {
    async fn check_permission(
        &self,
        _check: &PermissionCheck,
        _consistency_token: Option<&str>,
    ) -> Result<PolicyCheckResult, PolicyEngineError> {
        let decision = match self.decision.load(Ordering::SeqCst) {
            1 => PolicyDecision::Allowed,
            _ => PolicyDecision::Denied,
        };
        Ok(PolicyCheckResult {
            decision,
            checked_at_token: "check-revision".to_owned(),
        })
    }

    async fn bulk_check_permissions(
        &self,
        _checks: &[PermissionCheck],
        _consistency_token: Option<&str>,
    ) -> Result<PolicyBulkCheckResult, PolicyEngineError> {
        unreachable!()
    }

    async fn write_relationships(
        &self,
        updates: &[Relationship],
        deletions: &[Relationship],
    ) -> Result<PolicyWriteResult, PolicyEngineError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.batches
            .lock()
            .unwrap()
            .push((updates.to_vec(), deletions.to_vec()));
        Ok(PolicyWriteResult {
            written_at_token: "write-revision".to_owned(),
        })
    }
}

#[derive(Default)]
struct MemoryStore(Mutex<ReceiptEntries>);

type ReceiptEntries = HashMap<(WorkloadIdentity, String), ([u8; 32], MutationResult)>;

#[async_trait]
impl CommandReceiptStore for MemoryStore {
    async fn claim(&self, request: ReceiptClaimRequest) -> Result<ReceiptClaim, ReceiptStoreError> {
        let key = (request.caller, request.idempotency_key.clone());
        match self.0.lock().unwrap().get(&key).cloned() {
            Some((fingerprint, result)) if fingerprint == request.fingerprint => {
                Ok(ReceiptClaim::Replay(result))
            }
            Some(_) => Ok(ReceiptClaim::Conflict),
            None => Ok(ReceiptClaim::Acquired(ReceiptLease {
                caller: request.caller,
                idempotency_key: request.idempotency_key,
                fingerprint: request.fingerprint,
                lease_token: "lease".to_owned(),
                attempt: 1,
                recovered: false,
            })),
        }
    }

    async fn complete(
        &self,
        lease: &ReceiptLease,
        result: &MutationResult,
    ) -> Result<(), ReceiptStoreError> {
        self.0.lock().unwrap().insert(
            (lease.caller, lease.idempotency_key.clone()),
            (lease.fingerprint, result.clone()),
        );
        Ok(())
    }
}

async fn server() -> (String, Arc<FakePolicy>) {
    let policy = Arc::new(FakePolicy {
        decision: AtomicI32::new(1),
        writes: AtomicUsize::new(0),
        batches: Mutex::new(vec![]),
    });
    let service = AuthzGrpcService::new(
        Arc::clone(&policy),
        Arc::new(MemoryStore::default()),
        [7; 32],
        [9; 32],
        Duration::from_secs(5),
    )
    .unwrap();
    let interceptor = WorkloadIdentityInterceptor::test(WorkloadIdentity::Auction, true).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(AuthzServiceServer::with_interceptor(service, interceptor))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    (format!("http://{address}"), policy)
}

#[tokio::test]
async fn auction_client_and_generated_client_share_the_authz_wire_contract() {
    let (endpoint, policy) = server().await;
    let auction = AuthzGrpcAdapter::connect(&endpoint, Duration::from_secs(1))
        .await
        .unwrap();
    let check = AuctionPermissionCheck {
        principal: Principal {
            kind: PrincipalKind::User,
            id: "u1_bidder".to_owned(),
        },
        resource_type: "auction".to_owned(),
        resource_id: "auction-1".to_owned(),
        permission: "place_bid".to_owned(),
    };
    assert_eq!(
        auction.check(check.clone()).await.unwrap(),
        AuthorizationDecision::Allowed
    );
    policy.decision.store(0, Ordering::SeqCst);
    assert_eq!(
        auction.check(check).await.unwrap(),
        AuthorizationDecision::Denied
    );

    auction
        .write_relationship(RelationshipWrite {
            operation: ReconciliationOperation {
                operation_id: "operation-1".to_owned(),
                resource_type: "auction".to_owned(),
                resource_id: "auction-1".to_owned(),
                relation: "participant".to_owned(),
                subject_type: "user".to_owned(),
                subject_id: "u1_bidder".to_owned(),
            },
            idempotency_key: "relationship-1".to_owned(),
        })
        .await
        .unwrap();

    let mut generated = AuthzServiceClient::connect(endpoint).await.unwrap();
    let mut deletion = Request::new(rpc::WriteRelationshipsRequest {
        updates: vec![],
        idempotency_key: "relationship-delete-1".to_owned(),
        deletions: vec![rpc::Relationship {
            resource: Some(rpc::ResourceReference {
                resource_type: "auction".to_owned(),
                resource_id: "auction-1".to_owned(),
            }),
            relation: "participant".to_owned(),
            subject: Some(rpc::PrincipalReference {
                principal_type: "user".to_owned(),
                principal_id: "u1_bidder".to_owned(),
            }),
        }],
    });
    deletion.set_timeout(Duration::from_secs(1));
    let response = generated
        .write_relationships(deletion)
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        response.outcome,
        Some(rpc::write_relationships_response::Outcome::Written(_))
    ));
    assert_eq!(policy.writes.load(Ordering::SeqCst), 2);
    let batches = policy.batches.lock().unwrap();
    assert_eq!(batches[0].0.len(), 1);
    assert_eq!(batches[1].1.len(), 1);
}

#[tokio::test]
async fn cross_owner_rejection_is_safe_and_does_not_write() {
    let (endpoint, policy) = server().await;
    let mut client = AuthzServiceClient::connect(endpoint).await.unwrap();
    let secret_id = "payment-secret-resource-42";
    let mut request = Request::new(rpc::WriteRelationshipsRequest {
        updates: vec![rpc::Relationship {
            resource: Some(rpc::ResourceReference {
                resource_type: "payment_profile".to_owned(),
                resource_id: secret_id.to_owned(),
            }),
            relation: "owner".to_owned(),
            subject: Some(rpc::PrincipalReference {
                principal_type: "user".to_owned(),
                principal_id: "u1_secret-principal".to_owned(),
            }),
        }],
        idempotency_key: "secret-command-key".to_owned(),
        deletions: vec![],
    });
    request.set_timeout(Duration::from_secs(1));
    let response = client
        .write_relationships(request)
        .await
        .unwrap()
        .into_inner();
    let rejection = match response.outcome {
        Some(rpc::write_relationships_response::Outcome::Rejection(rejection)) => rejection,
        other => panic!("unexpected response: {other:?}"),
    };
    assert_eq!(rejection.reason, "FORBIDDEN");
    assert!(!rejection.safe_message.contains(secret_id));
    assert!(!rejection.safe_message.contains("u1_secret-principal"));
    assert!(!rejection.safe_message.contains("secret-command-key"));
    assert_eq!(policy.writes.load(Ordering::SeqCst), 0);
}
