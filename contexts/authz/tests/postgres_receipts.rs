use std::{sync::Arc, time::Duration};

use gavel_authz::{
    adapters::outbound::PostgresReceiptStore,
    application::{
        CommandReceiptStore, MutationOperation, MutationResult, PrincipalRegistered, ReceiptClaim,
        ReceiptClaimRequest, ReceiptStoreError, WorkloadIdentity,
    },
};
use sqlx::{Executor, PgPool, Row, postgres::PgPoolOptions};

async fn pool() -> Option<PgPool> {
    let url = match std::env::var("AUTHZ_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("AUTHZ_TEST_DATABASE_URL is unset; skipping PostgreSQL integration test");
            return None;
        }
    };
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool.execute("TRUNCATE authz_command_receipts")
        .await
        .unwrap();
    Some(pool)
}

fn request(key: &str, fingerprint: u8) -> ReceiptClaimRequest {
    ReceiptClaimRequest {
        caller: WorkloadIdentity::Bff,
        idempotency_key: key.to_owned(),
        operation: MutationOperation::RegisterPrincipal,
        fingerprint: [fingerprint; 32],
    }
}

fn result() -> MutationResult {
    MutationResult::PrincipalRegistered(PrincipalRegistered {
        principal_id: "u1_opaque".to_owned(),
        written_at_token: "revision-safe".to_owned(),
    })
}

#[tokio::test]
async fn receipt_store_serializes_claim_replay_conflict_recovery_and_completion() {
    let Some(pool) = pool().await else {
        return;
    };
    let store =
        Arc::new(PostgresReceiptStore::new(pool.clone(), Duration::from_millis(80)).unwrap());

    let (left, right) = tokio::join!(
        store.claim(request("concurrent", 1)),
        store.claim(request("concurrent", 1))
    );
    let claims = [left.unwrap(), right.unwrap()];
    assert_eq!(
        claims
            .iter()
            .filter(|claim| matches!(claim, ReceiptClaim::Acquired(_)))
            .count(),
        1
    );
    assert_eq!(
        claims
            .iter()
            .filter(|claim| matches!(claim, ReceiptClaim::InProgress))
            .count(),
        1
    );

    let lease = match store.claim(request("complete", 2)).await.unwrap() {
        ReceiptClaim::Acquired(lease) => lease,
        other => panic!("unexpected claim: {other:?}"),
    };
    let mut wrong_lease = lease.clone();
    wrong_lease.lease_token = "00000000-0000-7000-8000-000000000099".to_owned();
    assert_eq!(
        store.complete(&wrong_lease, &result()).await,
        Err(ReceiptStoreError::InvalidLease)
    );
    let status: String =
        sqlx::query("SELECT status FROM authz_command_receipts WHERE idempotency_key = 'complete'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("status");
    assert_eq!(status, "PENDING");

    store.complete(&lease, &result()).await.unwrap();
    assert_eq!(
        store.claim(request("complete", 2)).await.unwrap(),
        ReceiptClaim::Replay(result())
    );
    assert_eq!(
        store.claim(request("complete", 3)).await.unwrap(),
        ReceiptClaim::Conflict
    );

    let payload: serde_json::Value = sqlx::query(
        "SELECT result_payload FROM authz_command_receipts WHERE idempotency_key = 'complete'",
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .get("result_payload");
    let serialized = payload.to_string();
    for forbidden in [
        "issuer",
        "subject",
        "resource_id",
        "relationship",
        "idempotency_key",
    ] {
        assert!(!serialized.contains(forbidden));
    }

    let initial = match store.claim(request("recover", 4)).await.unwrap() {
        ReceiptClaim::Acquired(lease) => lease,
        other => panic!("unexpected claim: {other:?}"),
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        store.claim(request("recover", 5)).await.unwrap(),
        ReceiptClaim::Conflict
    );
    let recovered = match store.claim(request("recover", 4)).await.unwrap() {
        ReceiptClaim::Acquired(lease) => lease,
        other => panic!("unexpected recovery: {other:?}"),
    };
    assert!(recovered.recovered);
    assert_eq!(recovered.attempt, initial.attempt + 1);
}
