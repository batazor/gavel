use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use gavel_catalog::{
    adapters::outbound::postgres::PostgresCatalogStore,
    application::{
        AuctionEventInbox, CatalogOutbox, FreezeFromAuction, FreezeInboxOutcome,
        GrantReconciliationQueue, MediaRepository,
    },
    domain::{ImageId, LotId},
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};

async fn pool() -> Option<PgPool> {
    let url = match std::env::var("CATALOG_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("CATALOG_TEST_DATABASE_URL is unset; skipping PostgreSQL integration test");
            return None;
        }
    };
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .expect("test PostgreSQL must be reachable");
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "TRUNCATE catalog_reconciliation_queue, catalog_inbox, catalog_outbox, \
         catalog_command_receipts, catalog_lot_images, catalog_lots CASCADE",
    )
    .execute(&pool)
    .await
    .unwrap();
    Some(pool)
}

async fn insert_lot(pool: &PgPool, id: &str) {
    sqlx::query(
        "INSERT INTO catalog_lots \
         (lot_id, seller_id, title, start_price_minor, currency_code, bid_step_minor, \
          scheduled_end_millis, state, version, authz_revision, listed_at) \
         VALUES ($1, 'seller-1', 'Lot', 100, 'USD', 10, 2000, 'LISTED', 1, \
                 'revision', to_timestamp(1))",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

fn fingerprint(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

#[tokio::test]
async fn auction_inbox_is_monotonic_idempotent_and_quarantines_changed_payload() {
    let Some(pool) = pool().await else {
        return;
    };
    insert_lot(&pool, "lot-1").await;
    let store = PostgresCatalogStore::new(pool.clone());
    let applied = FreezeFromAuction {
        event_id: "event-7".to_owned(),
        payload_fingerprint: fingerprint("payload-7"),
        lot_id: LotId::new("lot-1").unwrap(),
        source_version: 7,
    };
    assert_eq!(
        store.freeze_from_first_bid(applied.clone()).await.unwrap(),
        FreezeInboxOutcome::Applied
    );
    assert_eq!(
        store.freeze_from_first_bid(applied.clone()).await.unwrap(),
        FreezeInboxOutcome::Duplicate
    );
    assert_eq!(
        store
            .freeze_from_first_bid(FreezeFromAuction {
                event_id: "event-6".to_owned(),
                payload_fingerprint: fingerprint("payload-6"),
                source_version: 6,
                ..applied.clone()
            })
            .await
            .unwrap(),
        FreezeInboxOutcome::Stale
    );
    assert_eq!(
        store
            .freeze_from_first_bid(FreezeFromAuction {
                payload_fingerprint: fingerprint("changed"),
                ..applied
            })
            .await
            .unwrap(),
        FreezeInboxOutcome::Quarantined
    );
    let row = sqlx::query(
        "SELECT description_frozen, auction_source_version FROM catalog_lots WHERE lot_id = 'lot-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(row.get::<bool, _>("description_frozen"));
    assert_eq!(row.get::<i64, _>("auction_source_version"), 7);
    let quarantined: String =
        sqlx::query_scalar("SELECT status FROM catalog_inbox WHERE event_id = 'event-7'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(quarantined, "QUARANTINED");
}

#[tokio::test]
async fn media_and_authz_leases_recover_and_never_claim_attached_or_existing_lot_grants() {
    let Some(pool) = pool().await else {
        return;
    };
    insert_lot(&pool, "lot-existing").await;
    sqlx::query(
        "INSERT INTO catalog_lot_images \
         (image_id, owner_id, object_key, expected_content_type, expected_content_length, \
          expected_checksum_sha256, expires_at, state) VALUES \
         ('expired', 'seller-1', 'private/expired', 'image/png', 42, $1, to_timestamp(1), 'PENDING')",
    )
    .bind(base64::engine::general_purpose::STANDARD.encode([1_u8; 32]))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO catalog_lot_images \
         (image_id, owner_id, object_key, expected_content_type, expected_content_length, \
          expected_checksum_sha256, expires_at, state, ready_at, lot_id, position, alt_text) \
         VALUES ('attached', 'seller-1', 'private/attached', 'image/png', 42, $1, \
                 to_timestamp(1), 'ATTACHED', to_timestamp(1), 'lot-existing', 0, '')",
    )
    .bind(base64::engine::general_purpose::STANDARD.encode([2_u8; 32]))
    .execute(&pool)
    .await
    .unwrap();
    let store = PostgresCatalogStore::new(pool.clone());
    let (left, right) = tokio::join!(
        store.lease_cleanup("worker-a", 2_000, 100, 500, 10),
        store.lease_cleanup("worker-b", 2_000, 100, 500, 10)
    );
    let mut leases = left.unwrap();
    leases.extend(right.unwrap());
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].image.image_id().as_str(), "expired");
    store
        .release_cleanup(leases[0].image.image_id(), &leases[0].token)
        .await
        .unwrap();
    let retry = store
        .lease_cleanup("worker-c", 2_000, 100, 500, 10)
        .await
        .unwrap();
    assert_eq!(retry.len(), 1);
    store
        .complete_cleanup(&ImageId::new("expired").unwrap(), &retry[0].token)
        .await
        .unwrap();
    let attached: i64 =
        sqlx::query_scalar("SELECT count(*) FROM catalog_lot_images WHERE image_id = 'attached'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attached, 1);

    for (operation, resource) in [("existing", "lot-existing"), ("orphan", "lot-orphan")] {
        sqlx::query(
            "INSERT INTO catalog_reconciliation_queue \
             (operation_id, operation_type, resource_id, owner_id, idempotency_key, not_before) \
             VALUES ($1, 'AUTHZ_GRANT_DELETE', $2, 'seller-1', $3, to_timestamp(1))",
        )
        .bind(operation)
        .bind(resource)
        .bind(format!("delete-{operation}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    let first = store.lease_due("worker-a", 2_000, 500, 1).await.unwrap();
    assert_eq!(first.len(), 1);
    assert!(store.lot_exists(&first[0].lot_id).await.unwrap());
    let none = store.lease_due("worker-b", 2_000, 500, 1).await.unwrap();
    assert_eq!(
        none.len(),
        1,
        "the other operation remains independently claimable"
    );
    assert!(!store.lot_exists(&none[0].lot_id).await.unwrap());
    GrantReconciliationQueue::release(&store, &none[0].operation_id, &none[0].lease_token)
        .await
        .unwrap();
    let recovered = store.lease_due("worker-c", 3_000, 500, 10).await.unwrap();
    assert!(recovered.iter().any(|lease| lease.operation_id == "orphan"));
}

#[tokio::test]
async fn outbox_leases_recover_and_retention_never_deletes_unpublished_rows() {
    let Some(pool) = pool().await else {
        return;
    };
    insert_lot(&pool, "lot-1").await;
    for (id, version, published) in [
        ("00000000-0000-7000-8000-000000000301", 1_i64, true),
        ("00000000-0000-7000-8000-000000000302", 2_i64, false),
    ] {
        sqlx::query(
            "INSERT INTO catalog_outbox \
             (event_id, aggregate_id, aggregate_version, event_type, occurred_at, payload, \
              correlation_id, published_at) \
             VALUES ($1::uuid, 'lot-1', $2, 'LotUpdated', to_timestamp(1), $3, 'correlation', \
                     CASE WHEN $4 THEN to_timestamp(1) ELSE NULL END)",
        )
        .bind(id)
        .bind(version)
        .bind(b"{}".as_slice())
        .bind(published)
        .execute(&pool)
        .await
        .unwrap();
    }
    let store = PostgresCatalogStore::new(pool.clone());
    let leased = CatalogOutbox::lease(&store, "worker-a", 2_000, 500, 10)
        .await
        .unwrap();
    assert_eq!(leased.len(), 1);
    CatalogOutbox::release(&store, &leased[0].event_id, &leased[0].lease_token)
        .await
        .unwrap();
    let recovered = CatalogOutbox::lease(&store, "worker-b", 2_000, 500, 10)
        .await
        .unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(store.delete_published_before(2_000).await.unwrap(), 1);
    let unpublished: i64 =
        sqlx::query_scalar("SELECT count(*) FROM catalog_outbox WHERE published_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unpublished, 1);
}
