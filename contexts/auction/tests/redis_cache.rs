use std::{sync::Arc, time::Duration};

use gavel_auction::{
    adapters::outbound::redis::{PriceChangedProjector, ProjectOutcome, RedisPriceCache},
    application::{AuctionCache, PriceSnapshot},
    rpc::{self, AuctionPublicEvent, PriceChangedEvent, auction_public_event::Payload},
};
use prost::Message;
use uuid::Uuid;

async fn cache() -> Option<RedisPriceCache> {
    let url = match std::env::var("AUCTION_TEST_REDIS_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("AUCTION_TEST_REDIS_URL is unset; skipping Redis integration test");
            return None;
        }
    };
    Some(
        RedisPriceCache::connect(
            &url,
            format!("gavel:test:{}:", Uuid::now_v7()),
            Duration::from_millis(200),
            2,
            Duration::from_millis(100),
            Duration::from_secs(60),
        )
        .await
        .expect("test Redis must be reachable"),
    )
}

fn snapshot(version: u64) -> PriceSnapshot {
    PriceSnapshot {
        auction_id: "auction-1".to_owned(),
        amount_minor: version * 100,
        currency_code: "USD".to_owned(),
        version,
        updated_at_millis: 1_700_000_000_000 + version,
    }
}

fn price_event(version: u64) -> Vec<u8> {
    AuctionPublicEvent {
        event_id: Uuid::now_v7().to_string(),
        aggregate_id: "auction-1".to_owned(),
        aggregate_version: version,
        schema_version: 1,
        occurred_at: Some(prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        }),
        correlation_id: "correlation-1".to_owned(),
        causation_id: "request-1".to_owned(),
        payload: Some(Payload::PriceChanged(PriceChangedEvent {
            price: Some(rpc::Money {
                minor_units: version * 100,
                currency_code: "USD".to_owned(),
            }),
            ends_at: Some(prost_types::Timestamp {
                seconds: 1_700_000_100,
                nanos: 0,
            }),
        })),
    }
    .encode_to_vec()
}

#[tokio::test]
async fn lua_compare_and_projector_are_monotonic_under_duplicates_and_reordering() {
    let Some(cache) = cache().await else {
        return;
    };
    assert!(cache.put_if_newer(snapshot(2)).await.unwrap());
    assert!(!cache.put_if_newer(snapshot(1)).await.unwrap());
    assert!(!cache.put_if_newer(snapshot(2)).await.unwrap());
    assert_eq!(cache.get("auction-1").await.unwrap().unwrap().version, 2);

    let (three, four) = tokio::join!(
        cache.put_if_newer(snapshot(3)),
        cache.put_if_newer(snapshot(4))
    );
    three.unwrap();
    four.unwrap();
    assert_eq!(cache.get("auction-1").await.unwrap().unwrap().version, 4);

    let cache = Arc::new(cache);
    let projector = PriceChangedProjector::new(Arc::clone(&cache));
    assert_eq!(
        projector.handle(&price_event(5)).await.unwrap(),
        ProjectOutcome::Applied
    );
    assert_eq!(
        projector.handle(&price_event(5)).await.unwrap(),
        ProjectOutcome::Ignored
    );
    assert_eq!(
        projector.handle(&price_event(3)).await.unwrap(),
        ProjectOutcome::Ignored
    );
    let stored = cache.get("auction-1").await.unwrap().unwrap();
    assert_eq!((stored.version, stored.amount_minor), (5, 500));
}
