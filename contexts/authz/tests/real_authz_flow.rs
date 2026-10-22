use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use gavel_authz::{
    adapters::{
        inbound::{AuthzGrpcService, WorkloadIdentityInterceptor},
        outbound::{PostgresReceiptStore, SpiceDbAdapter},
    },
    application::{
        PermissionCheck, PolicyBulkCheckResult, PolicyCheckResult, PolicyEngine, PolicyEngineError,
        PolicyWriteResult, Relationship, WorkloadIdentity,
    },
    rpc::{
        self, authz_service_client::AuthzServiceClient, authz_service_server::AuthzServiceServer,
    },
};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, transport::Server};

#[derive(Clone)]
struct RecoverablePolicy {
    inner: SpiceDbAdapter,
    fail_after_first_write: Arc<AtomicBool>,
}

#[async_trait]
impl PolicyEngine for RecoverablePolicy {
    async fn check_permission(
        &self,
        check: &PermissionCheck,
        token: Option<&str>,
    ) -> Result<PolicyCheckResult, PolicyEngineError> {
        self.inner.check_permission(check, token).await
    }

    async fn bulk_check_permissions(
        &self,
        checks: &[PermissionCheck],
        token: Option<&str>,
    ) -> Result<PolicyBulkCheckResult, PolicyEngineError> {
        self.inner.bulk_check_permissions(checks, token).await
    }

    async fn write_relationships(
        &self,
        updates: &[Relationship],
        deletions: &[Relationship],
    ) -> Result<PolicyWriteResult, PolicyEngineError> {
        let written = self.inner.write_relationships(updates, deletions).await?;
        if self.fail_after_first_write.swap(false, Ordering::SeqCst) {
            return Err(PolicyEngineError::Unavailable);
        }
        Ok(written)
    }
}

async fn start_server(
    policy: Arc<RecoverablePolicy>,
    receipts: Arc<PostgresReceiptStore>,
    identity: WorkloadIdentity,
) -> String {
    let service =
        AuthzGrpcService::new(policy, receipts, [17; 32], [29; 32], Duration::from_secs(5))
            .unwrap();
    let interceptor = WorkloadIdentityInterceptor::test(identity, true).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(AuthzServiceServer::with_interceptor(service, interceptor))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    format!("http://{address}")
}

fn registration(subject: &str, key: &str) -> Request<rpc::RegisterPrincipalRequest> {
    let mut request = Request::new(rpc::RegisterPrincipalRequest {
        issuer: "https://identity.example.test/".to_owned(),
        subject: subject.to_owned(),
        idempotency_key: key.to_owned(),
    });
    request.set_timeout(Duration::from_secs(2));
    request
}

fn relationship(principal_id: &str) -> rpc::Relationship {
    rpc::Relationship {
        resource: Some(rpc::ResourceReference {
            resource_type: "auction".to_owned(),
            resource_id: "real-flow-auction".to_owned(),
        }),
        relation: "participant".to_owned(),
        subject: Some(rpc::PrincipalReference {
            principal_type: "user".to_owned(),
            principal_id: principal_id.to_owned(),
        }),
    }
}

fn write_request(
    updates: Vec<rpc::Relationship>,
    deletions: Vec<rpc::Relationship>,
    key: &str,
) -> Request<rpc::WriteRelationshipsRequest> {
    let mut request = Request::new(rpc::WriteRelationshipsRequest {
        updates,
        idempotency_key: key.to_owned(),
        deletions,
    });
    request.set_timeout(Duration::from_secs(2));
    request
}

fn check_request(principal_id: &str, token: &str) -> Request<rpc::CheckPermissionRequest> {
    let mut request = Request::new(rpc::CheckPermissionRequest {
        check: Some(rpc::PermissionCheck {
            resource: Some(rpc::ResourceReference {
                resource_type: "auction".to_owned(),
                resource_id: "real-flow-auction".to_owned(),
            }),
            permission: "view_personal_state".to_owned(),
            principal: Some(rpc::PrincipalReference {
                principal_type: "user".to_owned(),
                principal_id: principal_id.to_owned(),
            }),
        }),
        consistency_token: token.to_owned(),
    });
    request.set_timeout(Duration::from_secs(2));
    request
}

fn registered(response: rpc::RegisterPrincipalResponse) -> rpc::PrincipalRegistered {
    match response.outcome {
        Some(rpc::register_principal_response::Outcome::Registered(value)) => value,
        other => panic!("expected registered result, got {other:?}"),
    }
}

fn written(response: rpc::WriteRelationshipsResponse) -> rpc::RelationshipsWritten {
    match response.outcome {
        Some(rpc::write_relationships_response::Outcome::Written(value)) => value,
        other => panic!("expected written result, got {other:?}"),
    }
}

async fn database() -> Option<PgPool> {
    let url = match std::env::var("AUTHZ_TEST_DATABASE_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("AUTHZ_TEST_DATABASE_URL is unset; skipping real Authz flow");
            return None;
        }
    };
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .expect("test PostgreSQL must be reachable");
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("TRUNCATE authz_command_receipts")
        .execute(&pool)
        .await
        .unwrap();
    Some(pool)
}

#[tokio::test]
async fn real_grpc_postgres_and_spicedb_cover_causal_lifecycle_and_receipts() {
    let Some(pool) = database().await else {
        return;
    };
    let spicedb_endpoint = match std::env::var("AUTHZ_TEST_SPICEDB_ENDPOINT") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("AUTHZ_TEST_SPICEDB_ENDPOINT is unset; skipping real Authz flow");
            return;
        }
    };
    let spicedb_token = std::env::var("AUTHZ_TEST_SPICEDB_TOKEN")
        .expect("AUTHZ_TEST_SPICEDB_TOKEN must accompany the endpoint");
    let spicedb =
        SpiceDbAdapter::connect(&spicedb_endpoint, &spicedb_token, Duration::from_secs(2))
            .await
            .unwrap();
    let policy = Arc::new(RecoverablePolicy {
        inner: spicedb,
        fail_after_first_write: Arc::new(AtomicBool::new(true)),
    });
    let receipts =
        Arc::new(PostgresReceiptStore::new(pool.clone(), Duration::from_millis(80)).unwrap());
    let bff_endpoint = start_server(
        Arc::clone(&policy),
        Arc::clone(&receipts),
        WorkloadIdentity::Bff,
    )
    .await;
    let auction_endpoint = start_server(policy, receipts, WorkloadIdentity::Auction).await;
    let mut bff = AuthzServiceClient::connect(bff_endpoint).await.unwrap();
    let mut auction = AuthzServiceClient::connect(auction_endpoint).await.unwrap();

    let first = bff
        .register_principal(registration("real-user", "register-recovery"))
        .await
        .unwrap_err();
    assert_eq!(first.code(), tonic::Code::Unavailable);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let principal = registered(
        bff.register_principal(registration("real-user", "register-recovery"))
            .await
            .unwrap()
            .into_inner(),
    );
    assert!(principal.principal_id.starts_with("u1_"));
    assert!(!principal.principal_id.contains("real-user"));
    assert!(!principal.written_at_token.is_empty());

    let receipt = sqlx::query(
        "SELECT status, attempts, result_payload::text AS payload FROM authz_command_receipts \
         WHERE caller_scope = 'bff' AND idempotency_key = 'register-recovery'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receipt.get::<String, _>("status"), "COMPLETED");
    assert_eq!(receipt.get::<i32, _>("attempts"), 2);
    let payload = receipt.get::<String, _>("payload");
    assert!(!payload.contains("real-user"));
    assert!(!payload.contains("identity.example.test"));

    let mut concurrent_left = bff.clone();
    let mut concurrent_right = bff.clone();
    let (left, right) = tokio::join!(
        concurrent_left.register_principal(registration("parallel-user", "register-parallel")),
        concurrent_right.register_principal(registration("parallel-user", "register-parallel"))
    );
    let left = registered(left.unwrap().into_inner());
    let right = registered(right.unwrap().into_inner());
    assert_eq!(left, right);
    let conflict = bff
        .register_principal(registration("different-user", "register-parallel"))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        conflict.outcome,
        Some(rpc::register_principal_response::Outcome::Rejection(ref value))
            if value.reason == "idempotency_conflict"
    ));

    let tuple = relationship(&principal.principal_id);
    let touch = written(
        auction
            .write_relationships(write_request(
                vec![tuple.clone()],
                vec![],
                "participant-touch",
            ))
            .await
            .unwrap()
            .into_inner(),
    );
    let allowed = auction
        .check_permission(check_request(
            &principal.principal_id,
            &touch.written_at_token,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(allowed.decision, rpc::PermissionDecision::Allowed as i32);
    assert!(!allowed.checked_at_token.is_empty());

    let deleted = written(
        auction
            .write_relationships(write_request(vec![], vec![tuple], "participant-delete"))
            .await
            .unwrap()
            .into_inner(),
    );
    assert_ne!(deleted.written_at_token, touch.written_at_token);
    let denied = auction
        .check_permission(check_request(
            &principal.principal_id,
            &deleted.written_at_token,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(denied.decision, rpc::PermissionDecision::Denied as i32);

    let forbidden = auction
        .write_relationships(write_request(
            vec![rpc::Relationship {
                resource: Some(rpc::ResourceReference {
                    resource_type: "payment_profile".to_owned(),
                    resource_id: "cross-owner".to_owned(),
                }),
                relation: "owner".to_owned(),
                subject: Some(rpc::PrincipalReference {
                    principal_type: "user".to_owned(),
                    principal_id: principal.principal_id,
                }),
            }],
            vec![],
            "cross-owner-write",
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        forbidden.outcome,
        Some(rpc::write_relationships_response::Outcome::Rejection(ref value))
            if value.reason == "FORBIDDEN"
    ));
    let forbidden_receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM authz_command_receipts WHERE idempotency_key = 'cross-owner-write'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(forbidden_receipts, 0);
}
