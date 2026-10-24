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

    pool.execute(
        "INSERT INTO auction_process_outbox \
         (event_id, aggregate_id, aggregate_version, event_type, schema_version, occurred_at, \
          payload, correlation_id, causation_id) \
         VALUES ('00000000-0000-7000-8000-000000000010', 'auction-1', 1, \
         'SettlementCandidateSelected', 1, clock_timestamp(), decode('01', 'hex'), \
         'correlation-1', 'causation-1')",
    )
    .await
    .unwrap();

    let duplicate_process_version = pool
        .execute(
            "INSERT INTO auction_process_outbox \
             (event_id, aggregate_id, aggregate_version, event_type, schema_version, occurred_at, \
              payload, correlation_id, causation_id) \
             VALUES ('00000000-0000-7000-8000-000000000011', 'auction-1', 1, \
             'SettlementCandidateSelected', 1, clock_timestamp(), decode('02', 'hex'), \
             'correlation-2', 'causation-2')",
        )
        .await;
    assert!(duplicate_process_version.is_err());

    let unsupported_process_event = pool
        .execute(
            "INSERT INTO auction_process_outbox \
             (event_id, aggregate_id, aggregate_version, event_type, schema_version, occurred_at, \
              payload, correlation_id, causation_id) \
             VALUES ('00000000-0000-7000-8000-000000000012', 'auction-1', 2, \
             'MaximumBidPlaced', 1, clock_timestamp(), decode('03', 'hex'), \
             'correlation-3', 'causation-3')",
        )
        .await;
    assert!(unsupported_process_event.is_err());

    let empty_process_payload = pool
        .execute(
            "INSERT INTO auction_process_outbox \
             (event_id, aggregate_id, aggregate_version, event_type, schema_version, occurred_at, \
              payload, correlation_id, causation_id) \
             VALUES ('00000000-0000-7000-8000-000000000013', 'auction-1', 2, \
             'AuctionUnsold', 1, clock_timestamp(), ''::bytea, \
             'correlation-4', 'causation-4')",
        )
        .await;
    assert!(empty_process_payload.is_err());
}
