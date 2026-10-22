use gavel_payments::{
    adapters::outbound::PostgresPayments,
    application::{
        CallbackClaim, CallbackInbox, Commit, CommitConflict, PaymentsUnitOfWork, ReceiptClaim,
        ReceiptClaimResult, ReceiptStore,
    },
    domain::{
        DebitAuthorization, DomainEvent, EmailAddress, PaymentInstrument, PaymentInstrumentId,
        PrincipalId, ProviderReference, RedactedDisplay, TermsVersion,
    },
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn migrations_create_constraints_and_outbox() {
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    PostgresPayments::new(pool.clone()).migrate().await.unwrap();
    let names: Vec<String> = sqlx::query_scalar("SELECT tablename FROM pg_tables WHERE schemaname='public' AND tablename LIKE 'payment_%' ORDER BY tablename").fetch_all(&pool).await.unwrap();
    assert!(names.contains(&"payment_attempts".to_owned()));
    assert!(names.contains(&"payment_command_receipts".to_owned()));
    assert!(names.contains(&"payment_callback_inbox".to_owned()));
    assert!(names.contains(&"payment_outbox".to_owned()));
}

async fn repository() -> PostgresPayments {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let repository = PostgresPayments::new(pool);
    repository.migrate().await.unwrap();
    repository
}

fn pending(suffix: &str) -> PaymentInstrument {
    PaymentInstrument::pending(
        PaymentInstrumentId::parse(format!("instrument-{suffix}")).unwrap(),
        PrincipalId::parse(format!("user-{suffix}")).unwrap(),
        EmailAddress::parse(format!("owner-{suffix}@example.test")).unwrap(),
        DebitAuthorization::new(TermsVersion::parse("2026-10").unwrap(), 1),
        ProviderReference::parse(format!("session-{suffix}")).unwrap(),
        1,
    )
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn instrument_uow_is_atomic_and_optimistically_versioned() {
    let repository = repository().await;
    let suffix = uuid::Uuid::now_v7().simple().to_string();
    let value = pending(&suffix);
    repository
        .save_instrument(
            Commit {
                value: value.clone(),
                events: vec![],
            },
            None,
        )
        .await
        .unwrap();
    let mut ready = value.clone();
    ready
        .mark_ready(
            ProviderReference::parse("customer-1").unwrap(),
            ProviderReference::parse("method-1").unwrap(),
            RedactedDisplay::parse("card").unwrap(),
            2,
        )
        .unwrap();
    let event = DomainEvent::PaymentInstrumentReady {
        event_id: format!("event-{suffix}"),
        instrument_id: ready.id().clone(),
        owner_id: ready.owner_id().clone(),
        version: ready.version(),
        occurred_at_millis: 2,
    };
    repository
        .save_instrument(
            Commit {
                value: ready.clone(),
                events: vec![event.clone()],
            },
            Some(1),
        )
        .await
        .unwrap();
    assert_eq!(
        repository
            .instrument_for_owner(ready.owner_id())
            .await
            .unwrap()
            .unwrap()
            .state(),
        ready.state()
    );
    assert_eq!(
        repository
            .save_instrument(
                Commit {
                    value: ready,
                    events: vec![event]
                },
                Some(1)
            )
            .await,
        Err(CommitConflict::Version)
    );
    let outbox: i64 = sqlx::query_scalar("SELECT count(*) FROM payment_outbox WHERE event_id=$1")
        .bind(format!("event-{suffix}"))
        .fetch_one(repository.pool())
        .await
        .unwrap();
    assert_eq!(outbox, 1);
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM payment_outbox WHERE event_id=$1")
            .bind(format!("event-{suffix}"))
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(payload["eventType"], "PaymentInstrumentReady");
    assert_eq!(payload["schemaVersion"], 1);
    assert_eq!(payload["payload"]["userId"], format!("user-{suffix}"));
    assert_eq!(
        payload["payload"]["instrumentId"],
        format!("instrument-{suffix}")
    );
    let serialized = payload.to_string();
    for forbidden in ["provider", "email", "recovery", "signature"] {
        assert!(!serialized.to_lowercase().contains(forbidden));
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn receipt_and_callback_inbox_handle_concurrency_replay_conflict_and_recovery() {
    let repository = Arc::new(repository().await);
    let suffix = uuid::Uuid::now_v7().simple().to_string();
    let claim = ReceiptClaim {
        scope: format!("scope-{suffix}"),
        command: "pay",
        key_digest: "key-digest".to_owned(),
        fingerprint: "fingerprint-a".to_owned(),
        lease_until_millis: u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap() + 30_000,
    };
    let mut tasks = vec![];
    for _ in 0..12 {
        let repository = repository.clone();
        let claim = claim.clone();
        tasks.push(tokio::spawn(async move {
            ReceiptStore::claim(&*repository, claim).await.unwrap()
        }));
    }
    let mut token = None;
    for task in tasks {
        match task.await.unwrap() {
            ReceiptClaimResult::Acquired { token: value } => {
                assert!(token.replace(value).is_none())
            }
            ReceiptClaimResult::InProgress => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
    let token = token.unwrap();
    ReceiptStore::complete(&*repository, &token, b"result")
        .await
        .unwrap();
    assert_eq!(
        ReceiptStore::claim(&*repository, claim.clone())
            .await
            .unwrap(),
        ReceiptClaimResult::Replay(b"result".to_vec())
    );
    let mut conflict = claim;
    conflict.fingerprint = "fingerprint-b".to_owned();
    assert_eq!(
        ReceiptStore::claim(&*repository, conflict).await.unwrap(),
        ReceiptClaimResult::Conflict
    );

    assert_eq!(
        CallbackInbox::claim(
            &*repository,
            "stripe",
            &format!("event-{suffix}"),
            "body-a",
            u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap() + 30_000
        )
        .await
        .unwrap(),
        CallbackClaim::Acquired
    );
    assert_eq!(
        CallbackInbox::claim(
            &*repository,
            "stripe",
            &format!("event-{suffix}"),
            "body-a",
            u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap() + 30_000
        )
        .await
        .unwrap(),
        CallbackClaim::InProgress
    );
    sqlx::query("UPDATE payment_callback_inbox SET lease_until=now() - interval '1 second' WHERE provider='stripe' AND event_ref_digest=$1")
        .bind(format!("event-{suffix}"))
        .execute(repository.pool())
        .await
        .unwrap();
    assert_eq!(
        CallbackInbox::claim(
            &*repository,
            "stripe",
            &format!("event-{suffix}"),
            "body-a",
            u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap() + 30_000
        )
        .await
        .unwrap(),
        CallbackClaim::Acquired
    );
    CallbackInbox::complete(&*repository, "stripe", &format!("event-{suffix}"))
        .await
        .unwrap();
    assert_eq!(
        CallbackInbox::claim(
            &*repository,
            "stripe",
            &format!("event-{suffix}"),
            "body-a",
            u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap() + 30_000
        )
        .await
        .unwrap(),
        CallbackClaim::Replay
    );
    assert_eq!(
        CallbackInbox::claim(
            &*repository,
            "stripe",
            &format!("event-{suffix}"),
            "body-b",
            u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap() + 30_000
        )
        .await
        .unwrap(),
        CallbackClaim::Conflict
    );
}
