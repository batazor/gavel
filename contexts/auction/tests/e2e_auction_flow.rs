use std::{sync::Arc, time::Duration};

use gavel_auction::{
    Timestamp,
    adapters::{
        inbound::events::{CatalogEventConsumer, ConsumeOutcome, PaymentsEventConsumer},
        outbound::{
            postgres::PostgresAuctionStore,
            redis::{PriceChangedProjector, RedisPriceCache},
        },
    },
    application::{
        AuctionCache, AuctionLifecycle, AuctionProjectionReader, AuctionUnitOfWork,
        CloseAuctionInput, CommandContext, ConfirmWinnerInput, ForfeitSettlementCandidateInput,
        LifecycleResult, PlaceBid, PlaceBidInput, Principal, PrincipalKind, PublicAuctionStatus,
        testing::StubAuthorization,
    },
    rpc::AuctionPublicEvent,
};
use prost::Message;
use serde_json::json;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;

struct Harness {
    pool: PgPool,
    store: Arc<PostgresAuctionStore>,
    cache: Arc<RedisPriceCache>,
}

async fn harness() -> Option<Harness> {
    let database_url = match std::env::var("AUCTION_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("AUCTION_TEST_DATABASE_URL is unset; skipping end-to-end test");
            return None;
        }
    };
    let redis_url = match std::env::var("AUCTION_TEST_REDIS_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("AUCTION_TEST_REDIS_URL is unset; skipping end-to-end test");
            return None;
        }
    };
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&database_url)
        .await
        .expect("test PostgreSQL must be reachable");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations must apply");
    let prefix = format!("gavel:e2e:{}:", Uuid::now_v7());
    let cache = RedisPriceCache::connect(
        &redis_url,
        prefix,
        Duration::from_millis(250),
        2,
        Duration::from_millis(100),
        Duration::from_secs(60),
    )
    .await
    .expect("test Redis must be reachable");
    let store = Arc::new(PostgresAuctionStore::new(pool.clone()));
    Some(Harness {
        pool,
        store,
        cache: Arc::new(cache),
    })
}

fn user_context(user_id: &str, key: String) -> CommandContext {
    CommandContext {
        principal: Principal {
            kind: PrincipalKind::User,
            id: user_id.to_owned(),
        },
        idempotency_key: key,
        correlation_id: "e2e-correlation".to_owned(),
        causation_id: "e2e-graphql-request".to_owned(),
    }
}

fn process_context(key: String) -> CommandContext {
    CommandContext {
        principal: Principal {
            kind: PrincipalKind::ServiceAccount,
            id: "processes".to_owned(),
        },
        idempotency_key: key,
        correlation_id: "e2e-correlation".to_owned(),
        causation_id: "e2e-workflow".to_owned(),
    }
}

#[tokio::test]
async fn lot_listed_to_confirmed_winner_is_atomic_idempotent_and_cache_safe() {
    let Some(harness) = harness().await else {
        return;
    };
    let suffix = Uuid::now_v7().to_string();
    let key = |label: &str| format!("e2e-{suffix}-{label}");
    let auction_id = format!("auction-{suffix}");
    let lot_id = format!("lot-{suffix}");
    let user_one = format!("bidder-one-{suffix}");
    let user_two = format!("bidder-two-{suffix}");
    let listed_at = 1_700_000_000_000_u64;
    let ends_at = listed_at + 60_000;

    let catalog_payload = serde_json::to_vec(&json!({
        "eventId": key("lot-listed"),
        "eventType": "LotListed",
        "schemaVersion": 1,
        "aggregateVersion": 1,
        "occurredAtMillis": listed_at,
        "correlationId": "e2e-correlation",
        "payload": {
            "auctionId": auction_id,
            "lotId": lot_id,
            "sellerId": format!("seller-{suffix}"),
            "startPriceMinor": 100,
            "currencyCode": "USD",
            "bidStepMinor": 10,
            "closingPolicy": {
                "scheduledEndMillis": ends_at,
                "inactivityMillis": null,
                "hardDeadlineMillis": null
            }
        }
    }))
    .unwrap();
    let catalog = CatalogEventConsumer::new(
        Arc::clone(&harness.store),
        Arc::clone(&harness.store),
        5,
        [11; 32],
    );
    assert_eq!(
        catalog.handle(&catalog_payload).await.unwrap(),
        ConsumeOutcome::Applied
    );
    assert_eq!(
        catalog.handle(&catalog_payload).await.unwrap(),
        ConsumeOutcome::Duplicate
    );

    let payments =
        PaymentsEventConsumer::new(Arc::clone(&harness.store), Arc::clone(&harness.store));
    for (position, user_id) in [&user_one, &user_two].into_iter().enumerate() {
        let payload = serde_json::to_vec(&json!({
            "eventId": key(&format!("payment-ready-{position}")),
            "eventType": "PaymentInstrumentReady",
            "schemaVersion": 1,
            "aggregateVersion": 1,
            "occurredAtMillis": listed_at + 500,
            "correlationId": "e2e-correlation",
            "payload": { "userId": user_id }
        }))
        .unwrap();
        assert_eq!(
            payments.handle(&payload).await.unwrap(),
            ConsumeOutcome::Applied
        );
    }

    let authorization = Arc::new(StubAuthorization::allowed());
    let bidding = PlaceBid::new(
        Arc::clone(&harness.store),
        Arc::clone(&authorization),
        Arc::clone(&harness.store),
        5,
        [22; 32],
    );
    let first_context = user_context(&user_one, key("bid-one"));
    let second_context = user_context(&user_two, key("bid-two"));
    let first_input = PlaceBidInput {
        auction_id: auction_id.clone(),
        bid_id: format!("bid-one-{suffix}"),
        maximum_minor: 300,
        currency_code: "USD".to_owned(),
        placed_at: Timestamp::from_millis(listed_at + 1_000),
    };
    let second_input = PlaceBidInput {
        auction_id: auction_id.clone(),
        bid_id: format!("bid-two-{suffix}"),
        maximum_minor: 200,
        currency_code: "USD".to_owned(),
        placed_at: Timestamp::from_millis(listed_at + 1_001),
    };
    let (first_result, second_result) = tokio::join!(
        bidding.handle(&first_context, first_input.clone()),
        bidding.handle(&second_context, second_input)
    );
    let first_result = first_result.expect("first concurrent bid must succeed after OCC retry");
    second_result.expect("second concurrent bid must succeed after OCC retry");

    let event_count_before_replay: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auction_events WHERE auction_id = $1")
            .bind(&auction_id)
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    let replay = bidding
        .handle(&first_context, first_input)
        .await
        .expect("exact duplicate bid must replay");
    assert_eq!(replay, first_result);
    let event_count_after_replay: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auction_events WHERE auction_id = $1")
            .bind(&auction_id)
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    assert_eq!(event_count_after_replay, event_count_before_replay);

    let lifecycle = AuctionLifecycle::new(Arc::clone(&harness.store), authorization, 5, [33; 32]);
    let close_context = process_context(key("close"));
    let close_input = CloseAuctionInput {
        auction_id: auction_id.clone(),
        now: Timestamp::from_millis(ends_at),
    };
    let closed = lifecycle
        .close(&close_context, close_input.clone())
        .await
        .expect("deadline close must succeed");
    let stream_count_after_close: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auction_events WHERE auction_id = $1")
            .bind(&auction_id)
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    assert_eq!(
        lifecycle.close(&close_context, close_input).await.unwrap(),
        closed
    );
    let stream_count_after_close_replay: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auction_events WHERE auction_id = $1")
            .bind(&auction_id)
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    assert_eq!(stream_count_after_close_replay, stream_count_after_close);

    let advanced = lifecycle
        .forfeit(
            &process_context(key("forfeit")),
            ForfeitSettlementCandidateInput {
                auction_id: auction_id.clone(),
                candidate_id: user_one.clone(),
                settlement_round: 1,
                occurred_at: Timestamp::from_millis(ends_at + 1_000),
            },
        )
        .await
        .expect("leading candidate forfeiture must advance settlement");
    assert!(matches!(
        advanced,
        LifecycleResult::SettlementAdvanced {
            settlement_round: 2,
            ..
        }
    ));
    assert!(matches!(
        lifecycle
            .confirm(
                &process_context(key("confirm")),
                ConfirmWinnerInput {
                    auction_id: auction_id.clone(),
                    candidate_id: user_two.clone(),
                    payment_id: "provider-payment-secret".to_owned(),
                    occurred_at: Timestamp::from_millis(ends_at + 2_000),
                },
            )
            .await
            .expect("second candidate must be confirmable"),
        LifecycleResult::WinnerConfirmed { .. }
    ));

    let loaded = harness.store.load(&auction_id).await.unwrap().unwrap();
    let versions: Vec<i64> = sqlx::query_scalar(
        "SELECT version FROM auction_events WHERE auction_id = $1 ORDER BY version",
    )
    .bind(&auction_id)
    .fetch_all(&harness.pool)
    .await
    .unwrap();
    assert_eq!(
        versions,
        (1..=i64::try_from(loaded.version).unwrap()).collect::<Vec<_>>()
    );
    let projection = harness.store.auction(&auction_id).await.unwrap().unwrap();
    assert_eq!(projection.view.status, PublicAuctionStatus::Sold);
    assert_eq!(projection.view.version, loaded.version);
    assert_eq!(projection.view.current_price.minor_units, 210);

    let command_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM auction_command_results WHERE idempotency_key LIKE $1",
    )
    .bind(format!("e2e-{suffix}-%"))
    .fetch_one(&harness.pool)
    .await
    .unwrap();
    assert_eq!(command_count, 6, "duplicates must not create command rows");
    let inbox_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auction_inbox WHERE event_id LIKE $1")
            .bind(format!("e2e-{suffix}-%"))
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    assert_eq!(inbox_count, 3, "duplicate delivery must reuse inbox row");

    let outbox = sqlx::query(
        "SELECT event_type, payload FROM auction_outbox WHERE aggregate_id = $1 \
         ORDER BY aggregate_version, occurred_at, event_id",
    )
    .bind(&auction_id)
    .fetch_all(&harness.pool)
    .await
    .unwrap();
    assert!(!outbox.is_empty());
    let projector = PriceChangedProjector::new(Arc::clone(&harness.cache));
    let mut latest_price_version = 0_u64;
    for row in outbox {
        let event_type: String = row.try_get("event_type").unwrap();
        let payload: Vec<u8> = row.try_get("payload").unwrap();
        let envelope = AuctionPublicEvent::decode(payload.as_slice()).unwrap();
        assert_eq!(envelope.aggregate_id, auction_id);
        let wire_text = String::from_utf8_lossy(&payload);
        for secret in [&user_one, &user_two, &"provider-payment-secret".to_owned()] {
            assert!(
                !wire_text.contains(secret.as_str()),
                "outbox leaked {secret}"
            );
        }
        assert!(!matches!(
            event_type.as_str(),
            "MaximumBidPlaced" | "SettlementCandidateSelected" | "SettlementCandidateForfeited"
        ));
        if event_type == "PriceChanged" {
            latest_price_version = latest_price_version.max(envelope.aggregate_version);
            projector.handle(&payload).await.unwrap();
            projector.handle(&payload).await.unwrap();
        }
    }
    assert!(latest_price_version > 0);
    let cached = harness.cache.get(&auction_id).await.unwrap().unwrap();
    assert_eq!(cached.version, latest_price_version);
    assert_eq!(cached.amount_minor, 210);
}
