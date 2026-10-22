use sqlx::{Executor, PgPool, postgres::PgPoolOptions};

async fn pool() -> Option<PgPool> {
    let url = match std::env::var("CATALOG_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("CATALOG_TEST_DATABASE_URL is unset; skipping PostgreSQL integration test");
            return None;
        }
    };
    Some(
        PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("test PostgreSQL must be reachable"),
    )
}

#[tokio::test]
async fn migrations_are_repeatable_and_constraints_reject_invalid_rows() {
    let Some(pool) = pool().await else {
        return;
    };
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("clean migration must apply");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("repeat migration must be a no-op");

    pool.execute(
        "INSERT INTO catalog_lots \
         (lot_id, seller_id, title, start_price_minor, currency_code, bid_step_minor, \
          scheduled_end_millis, state, version, authz_revision, listed_at) \
         VALUES ('lot-1', 'seller-1', 'Lot', 100, 'USD', 10, 2000, 'LISTED', 1, \
                 'revision', to_timestamp(1))",
    )
    .await
    .unwrap();
    let invalid_currency = pool
        .execute(
            "INSERT INTO catalog_lots \
             (lot_id, seller_id, title, start_price_minor, currency_code, bid_step_minor, \
              scheduled_end_millis, state, version, authz_revision, listed_at) \
             VALUES ('lot-invalid', 'seller-1', 'Lot', 100, 'usd', 10, 2000, 'LISTED', 1, \
                     'revision', to_timestamp(1))",
        )
        .await;
    assert!(invalid_currency.is_err());

    pool.execute(
        "INSERT INTO catalog_lot_images \
         (image_id, owner_id, object_key, expected_content_type, expected_content_length, \
          expected_checksum_sha256, expires_at, state, ready_at, lot_id, position, alt_text) \
         VALUES ('image-1', 'seller-1', 'private/image-1', 'image/png', 42, 'checksum', \
                 to_timestamp(2), 'ATTACHED', to_timestamp(1), 'lot-1', 0, '')",
    )
    .await
    .unwrap();
    let duplicate_position = pool
        .execute(
            "INSERT INTO catalog_lot_images \
             (image_id, owner_id, object_key, expected_content_type, expected_content_length, \
              expected_checksum_sha256, expires_at, state, ready_at, lot_id, position, alt_text) \
             VALUES ('image-2', 'seller-1', 'private/image-2', 'image/png', 42, 'checksum', \
                     to_timestamp(2), 'ATTACHED', to_timestamp(1), 'lot-1', 0, '')",
        )
        .await;
    assert!(duplicate_position.is_err());

    let invalid_receipt = pool
        .execute(
            "INSERT INTO catalog_command_receipts \
             (caller_scope, idempotency_key, command, request_fingerprint, status, claim_token) \
             VALUES ('seller-1', 'key-1', 'list_lot', decode(repeat('01', 31), 'hex'), \
                     'CLAIMED', '00000000-0000-7000-8000-000000000001')",
        )
        .await;
    assert!(invalid_receipt.is_err());

    let attached_without_lot = pool
        .execute(
            "INSERT INTO catalog_lot_images \
             (image_id, owner_id, object_key, expected_content_type, expected_content_length, \
              expected_checksum_sha256, expires_at, state, ready_at) \
             VALUES ('image-invalid', 'seller-1', 'private/invalid', 'image/png', 42, 'checksum', \
                     to_timestamp(2), 'ATTACHED', to_timestamp(1))",
        )
        .await;
    assert!(attached_without_lot.is_err());
}
