use std::sync::Arc;

use gavel_auction::{
    adapters::inbound::events::{
        CatalogEventConsumer, ConsumeOutcome, ConsumerError, PaymentsEventConsumer,
    },
    application::{
        AuctionUnitOfWork, PaymentReadiness,
        testing::{InMemoryAuctionStore, InMemoryInbox, InMemoryPaymentReadiness},
    },
};

const LOT_LISTED: &[u8] = include_bytes!("../../catalog/tests/fixtures/lot_listed_v1.json");
const PAYMENT_READY: &[u8] =
    include_bytes!("../../payments/tests/fixtures/payment_instrument_ready_v1.json");
const PAYMENT_REVOKED: &[u8] =
    include_bytes!("../../payments/tests/fixtures/payment_instrument_revoked_v1.json");

#[tokio::test]
async fn catalog_fixture_opens_once_and_conflicting_redelivery_is_quarantined() {
    let store = Arc::new(InMemoryAuctionStore::default());
    let inbox = Arc::new(InMemoryInbox::default());
    let consumer = CatalogEventConsumer::new(Arc::clone(&store), inbox, 3, [9; 32]);

    assert_eq!(
        consumer.handle(LOT_LISTED).await.unwrap(),
        ConsumeOutcome::Applied
    );
    assert_eq!(
        consumer.handle(LOT_LISTED).await.unwrap(),
        ConsumeOutcome::Duplicate
    );
    assert_eq!(store.load("auction-1").await.unwrap().unwrap().version, 1);

    let changed = String::from_utf8(LOT_LISTED.to_vec())
        .unwrap()
        .replace("\"startPriceMinor\": 100", "\"startPriceMinor\": 999");
    assert_eq!(
        consumer.handle(changed.as_bytes()).await.unwrap(),
        ConsumeOutcome::Quarantined
    );
    assert_eq!(store.load("auction-1").await.unwrap().unwrap().version, 1);
}

#[tokio::test]
async fn payment_fixtures_are_duplicate_safe_and_source_version_monotonic() {
    let inbox = Arc::new(InMemoryInbox::default());
    let readiness = Arc::new(InMemoryPaymentReadiness::default());
    let consumer = PaymentsEventConsumer::new(inbox, Arc::clone(&readiness));

    assert_eq!(
        consumer.handle(PAYMENT_READY).await.unwrap(),
        ConsumeOutcome::Applied
    );
    assert_eq!(
        consumer.handle(PAYMENT_READY).await.unwrap(),
        ConsumeOutcome::Duplicate
    );
    assert_eq!(
        consumer.handle(PAYMENT_REVOKED).await.unwrap(),
        ConsumeOutcome::Stale
    );
    assert!(readiness.is_ready("user-1").await.unwrap());
}

#[tokio::test]
async fn malformed_and_unknown_versions_are_rejected_without_panics() {
    let store = Arc::new(InMemoryAuctionStore::default());
    let consumer = CatalogEventConsumer::new(store, Arc::new(InMemoryInbox::default()), 3, [9; 32]);
    assert_eq!(
        consumer.handle(b"not-json").await,
        Err(ConsumerError::Malformed)
    );
    let unknown = String::from_utf8(LOT_LISTED.to_vec())
        .unwrap()
        .replace("\"schemaVersion\": 1", "\"schemaVersion\": 99");
    assert_eq!(
        consumer.handle(unknown.as_bytes()).await,
        Err(ConsumerError::Unsupported)
    );
}
