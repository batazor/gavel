use sqlx::{Executor, PgPool, postgres::PgPoolOptions};

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
            .max_connections(4)
            .connect(&url)
            .await
            .expect("test PostgreSQL must be reachable"),
    )
}

#[tokio::test]
async fn migrations_are_repeatable_and_constraints_protect_contracts() {
    let Some(pool) = pool().await else {
        return;
    };

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("fresh migration must apply");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("rerunning migrator must be a no-op");

    pool.execute("INSERT INTO auction_streams (auction_id) VALUES ('auction-1')")
        .await
        .unwrap();
    pool.execute(
        "INSERT INTO auction_events \
         (auction_id, version, event_id, event_type, event_schema_version, payload, occurred_at) \
         VALUES ('auction-1', 1, '00000000-0000-7000-8000-000000000001', \
         'AuctionOpened', 1, '{}'::jsonb, clock_timestamp())",
    )
    .await
    .unwrap();

    let duplicate_version = pool
        .execute(
            "INSERT INTO auction_events \
             (auction_id, version, event_id, event_type, event_schema_version, payload, occurred_at) \
             VALUES ('auction-1', 1, '00000000-0000-7000-8000-000000000002', \
             'AuctionOpened', 1, '{}'::jsonb, clock_timestamp())",
        )
        .await;
    assert!(duplicate_version.is_err());

    let invalid_currency = pool
        .execute(
            "INSERT INTO auction_public_projection \
             (auction_id, lot_id, status, amount_minor, currency_code, ends_at, stream_version) \
             VALUES ('auction-1', 'lot-1', 'OPEN', 100, 'usd', clock_timestamp(), 1)",
        )
        .await;
    assert!(invalid_currency.is_err());

    pool.execute(
        "INSERT INTO auction_inbox (consumer, event_id, payload_fingerprint) \
         VALUES ('catalog', 'event-1', decode(repeat('01', 32), 'hex'))",
    )
    .await
    .unwrap();
    let duplicate_inbox = pool
        .execute(
            "INSERT INTO auction_inbox (consumer, event_id, payload_fingerprint) \
             VALUES ('catalog', 'event-1', decode(repeat('02', 32), 'hex'))",
        )
        .await;
    assert!(duplicate_inbox.is_err());
}
