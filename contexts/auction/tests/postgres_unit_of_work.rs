use gavel_auction::{
    Auction, AuctionCommand, AuctionId, ClosingPolicy, LotId, Money, SellerId, Timestamp,
    adapters::outbound::postgres::PostgresAuctionStore,
    application::{
        AuctionProjection, AuctionProjectionReader, AuctionUnitOfWork, AuctionView,
        CommandIdentity, CommandResult, CommitError, CommitOutcome, CommitRequest, Inbox,
        InboxOutcome, MoneyView, PaymentReadiness, PaymentReadinessStore, PaymentReadinessUpdate,
        PaymentReadinessUpdateOutcome, PublicAuctionStatus, ReconciliationQueue,
    },
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    let url = match std::env::var("AUCTION_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("AUCTION_TEST_DATABASE_URL is unset; skipping PostgreSQL integration test");
            return None;
        }
    };
    Some(
        PgPoolOptions::new()
            .max_connections(8)
            .connect(&url)
            .await
            .expect("test PostgreSQL must be reachable"),
    )
}

fn open_request(auction_id: &str, key: &str) -> CommitRequest {
    let opened_at = Timestamp::from_millis(1_700_000_000_000);
    let ends_at = Timestamp::from_millis(1_700_000_060_000);
    let start_price = Money::new(100, "USD").unwrap();
    let decision = Auction::default()
        .decide(AuctionCommand::Open {
            auction_id: AuctionId::from(auction_id.to_owned()),
            lot_id: LotId::from(format!("lot-{auction_id}")),
            seller_id: SellerId::from("seller-1"),
            start_price: start_price.clone(),
            bid_step_minor: 10,
            closing_policy: ClosingPolicy::new(Some(ends_at), None, None).unwrap(),
            opened_at,
        })
        .unwrap();
    CommitRequest {
        auction_id: auction_id.to_owned(),
        expected_version: 0,
        private_events: decision.events,
        public_events: decision.public_events,
        command: Some((
            CommandIdentity {
                principal_scope: "service_account:catalog".to_owned(),
                idempotency_key: key.to_owned(),
                request_fingerprint: "11".repeat(32),
                operation: "open_auction_from_lot_listed".to_owned(),
                correlation_id: format!("correlation-{key}"),
                causation_id: format!("event-{key}"),
            },
            CommandResult::Opened { version: 1 },
        )),
        projection: Some(AuctionProjection {
            view: AuctionView {
                auction_id: auction_id.to_owned(),
                lot_id: format!("lot-{auction_id}"),
                status: PublicAuctionStatus::Open,
                current_price: MoneyView {
                    minor_units: 100,
                    currency_code: "USD".to_owned(),
                },
                ends_at_millis: ends_at.as_millis(),
                version: 1,
            },
            updated_at: opened_at,
        }),
        reconciliation: Vec::new(),
    }
}

#[tokio::test]
async fn postgres_uow_is_atomic_idempotent_and_optimistically_concurrent() {
    let Some(pool) = pool().await else {
        return;
    };
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations must apply");
    let store = PostgresAuctionStore::new(pool.clone());
    let suffix = Uuid::now_v7();

    let rollback_id = format!("rollback-{suffix}");
    let mut invalid = open_request(&rollback_id, &format!("rollback-{suffix}"));
    invalid
        .projection
        .as_mut()
        .unwrap()
        .view
        .current_price
        .currency_code = "usd".to_owned();
    assert_eq!(store.commit(invalid).await, Err(CommitError::Internal));
    for table in [
        "auction_streams",
        "auction_events",
        "auction_outbox",
        "auction_process_outbox",
        "auction_command_results",
    ] {
        let query = format!(
            "SELECT count(*) FROM {table} WHERE {} = $1",
            if table == "auction_command_results" {
                "idempotency_key"
            } else if matches!(table, "auction_outbox" | "auction_process_outbox") {
                "aggregate_id"
            } else {
                "auction_id"
            }
        );
        let lookup = if table == "auction_command_results" {
            format!("rollback-{suffix}")
        } else {
            rollback_id.clone()
        };
        let count: i64 = sqlx::query_scalar(&query)
            .bind(lookup)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "{table} must roll back with the transaction");
    }

    let replay_id = format!("replay-{suffix}");
    let replay_request = open_request(&replay_id, &format!("replay-{suffix}"));
    assert_eq!(
        store.commit(replay_request.clone()).await.unwrap(),
        CommitOutcome::Committed { version: 1 }
    );
    assert_eq!(
        store.commit(replay_request).await.unwrap(),
        CommitOutcome::Replayed {
            result: CommandResult::Opened { version: 1 }
        }
    );
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auction_events WHERE auction_id = $1")
            .bind(&replay_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let outbox_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auction_outbox WHERE aggregate_id = $1")
            .bind(&replay_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((event_count, outbox_count), (1, 1));
    let open_projection = store.auction(&replay_id).await.unwrap().unwrap();
    assert_eq!(open_projection.view.status, PublicAuctionStatus::Open);
    assert_eq!(open_projection.view.current_price.minor_units, 100);
    assert_eq!(open_projection.view.version, 1);

    sqlx::query(
        "INSERT INTO auction_snapshots \
         (auction_id, version, snapshot_schema_version, payload) VALUES ($1, 1, 999, '{}'::jsonb)",
    )
    .bind(&replay_id)
    .execute(&pool)
    .await
    .unwrap();
    let loaded = store.load(&replay_id).await.unwrap().unwrap();
    assert!(
        loaded.snapshot.is_none(),
        "unknown snapshots must be ignored"
    );
    assert_eq!(loaded.version, 1);
    assert_eq!(
        loaded.events.len(),
        1,
        "fallback must replay the full stream"
    );
    assert_eq!(
        Auction::rehydrate(loaded.events.clone()).unwrap().version(),
        1
    );

    let aggregate = Auction::rehydrate(loaded.events).unwrap();
    let cancelled = aggregate.decide(AuctionCommand::Cancel).unwrap();
    assert_eq!(
        store
            .commit(CommitRequest {
                auction_id: replay_id.clone(),
                expected_version: 1,
                private_events: cancelled.events,
                public_events: cancelled.public_events,
                command: None,
                projection: Some(AuctionProjection {
                    view: AuctionView {
                        auction_id: replay_id.clone(),
                        lot_id: format!("lot-{replay_id}"),
                        status: PublicAuctionStatus::Cancelled,
                        current_price: MoneyView {
                            minor_units: 100,
                            currency_code: "USD".to_owned(),
                        },
                        ends_at_millis: 1_700_000_060_000,
                        version: 2,
                    },
                    updated_at: Timestamp::from_millis(1_700_000_010_000),
                }),
                reconciliation: Vec::new(),
            })
            .await
            .unwrap(),
        CommitOutcome::Committed { version: 2 }
    );
    let terminal = store.auction(&replay_id).await.unwrap().unwrap();
    assert_eq!(terminal.view.status, PublicAuctionStatus::Cancelled);
    assert_eq!(terminal.view.version, 2);
    assert_eq!(terminal.view.current_price.minor_units, 100);
    let process_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auction_process_outbox WHERE aggregate_id = $1")
            .bind(&replay_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(process_count, 1, "cancel must emit one restricted fact");

    let concurrent_id = format!("concurrent-{suffix}");
    let left = open_request(&concurrent_id, &format!("left-{suffix}"));
    let right = open_request(&concurrent_id, &format!("right-{suffix}"));
    let (left_result, right_result) = tokio::join!(store.commit(left), store.commit(right));
    let results = [left_result, right_result];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(CommitOutcome::Committed { version: 1 })))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(CommitError::VersionConflict)))
            .count(),
        1
    );
    let versions: Vec<i64> = sqlx::query_scalar(
        "SELECT version FROM auction_events WHERE auction_id = $1 ORDER BY version",
    )
    .bind(&concurrent_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(versions, vec![1]);
}

#[tokio::test]
async fn postgres_consumer_state_is_duplicate_safe_monotonic_and_quarantines_conflicts() {
    let Some(pool) = pool().await else {
        return;
    };
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations must apply");
    let store = PostgresAuctionStore::new(pool.clone());
    let suffix = Uuid::now_v7();
    let event_id = format!("catalog-{suffix}");
    let fingerprint = "22".repeat(32);

    assert_eq!(
        store
            .accept("catalog", &event_id, &fingerprint)
            .await
            .unwrap(),
        InboxOutcome::Accepted
    );
    assert_eq!(
        store
            .accept("catalog", &event_id, &fingerprint)
            .await
            .unwrap(),
        InboxOutcome::Duplicate
    );
    assert_eq!(
        store
            .accept("catalog", &event_id, &"33".repeat(32))
            .await
            .unwrap(),
        InboxOutcome::PayloadConflict
    );
    let status: String = sqlx::query_scalar(
        "SELECT status FROM auction_inbox WHERE consumer = 'catalog' AND event_id = $1",
    )
    .bind(&event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "QUARANTINED");

    let concurrent_event = format!("payments-{suffix}");
    let (left, right) = tokio::join!(
        store.accept("payments", &concurrent_event, &fingerprint),
        store.accept("payments", &concurrent_event, &fingerprint),
    );
    let outcomes = [left.unwrap(), right.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == InboxOutcome::Accepted)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == InboxOutcome::Duplicate)
            .count(),
        1
    );

    let user_id = format!("user-{suffix}");
    let update = |ready, source_version, source_event_id: &str| PaymentReadinessUpdate {
        user_id: user_id.clone(),
        ready,
        source_version,
        source_event_id: source_event_id.to_owned(),
        occurred_at: Timestamp::from_millis(1_700_000_000_000 + source_version),
    };
    assert_eq!(
        store
            .apply_update(update(true, 2, "ready-v2"))
            .await
            .unwrap(),
        PaymentReadinessUpdateOutcome::Applied
    );
    assert_eq!(
        store
            .apply_update(update(false, 1, "revoked-v1"))
            .await
            .unwrap(),
        PaymentReadinessUpdateOutcome::Stale
    );
    assert!(store.is_ready(&user_id).await.unwrap());
    assert_eq!(
        store
            .apply_update(update(true, 2, "ready-v2"))
            .await
            .unwrap(),
        PaymentReadinessUpdateOutcome::Duplicate
    );
    assert_eq!(
        store
            .apply_update(update(false, 3, "revoked-v3"))
            .await
            .unwrap(),
        PaymentReadinessUpdateOutcome::Applied
    );
    assert!(!store.is_ready(&user_id).await.unwrap());

    let operation_id = format!("relationship-{suffix}");
    sqlx::query(
        "INSERT INTO auction_reconciliation_queue \
         (operation_id, resource_type, resource_id, relation, subject_type, subject_id) \
         VALUES ($1, 'auction', 'auction-1', 'participant', 'user', $2)",
    )
    .bind(&operation_id)
    .bind(&user_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        store
            .pending(100)
            .await
            .unwrap()
            .iter()
            .any(|operation| operation.operation_id == operation_id)
    );
    store.complete(&operation_id).await.unwrap();
    store.complete(&operation_id).await.unwrap();
    assert!(
        !store
            .pending(100)
            .await
            .unwrap()
            .iter()
            .any(|operation| operation.operation_id == operation_id)
    );
}
