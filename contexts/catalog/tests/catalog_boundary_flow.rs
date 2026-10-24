use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use gavel_auction::{
    AuctionEvent,
    adapters::inbound::events::{CatalogEventConsumer, ConsumeOutcome},
    application::{
        AuctionUnitOfWork,
        testing::{InMemoryAuctionStore, InMemoryInbox},
    },
};
use gavel_catalog::{
    adapters::outbound::{
        postgres::PostgresCatalogStore,
        postgres_reader::PostgresPublicReader,
        s3::{S3Config, S3ObjectStorage},
    },
    application::{
        Authorization, AuthorizationRevision, CatalogOutbox, CatalogReader, CleanupWorker, Clock,
        ClosingPolicyInput, CommandContext, CompleteLotImageUpload, CompleteUploadInput,
        IdGenerator, ImageAttachmentInput, ListLot, ListLotInput, LotOwnerGrant, LotQuery,
        MediaObservation, MediaTelemetry, MoneyInput, PermissionCheck, PermissionDecision,
        PortError, RequestLotImageUpload, RequestUploadInput, ServicePermissionCheck,
    },
    domain::ImageId,
};
use reqwest::Client;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};

async fn pool() -> Option<PgPool> {
    let url = std::env::var("CATALOG_TEST_DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "TRUNCATE catalog_outbox, catalog_reconciliation_queue, catalog_inbox, \
         catalog_command_receipts, catalog_lot_images, catalog_lots CASCADE",
    )
    .execute(&pool)
    .await
    .unwrap();
    Some(pool)
}

fn storage() -> Option<S3ObjectStorage> {
    let endpoint = std::env::var("CATALOG_TEST_S3_ENDPOINT").ok()?;
    S3ObjectStorage::new(S3Config {
        endpoint: endpoint.parse().unwrap(),
        region: "us-east-1".to_owned(),
        bucket: "catalog-images".to_owned(),
        access_key: std::env::var("CATALOG_TEST_S3_ACCESS_KEY").unwrap(),
        secret_key: std::env::var("CATALOG_TEST_S3_SECRET_KEY").unwrap(),
        session_token: None,
        path_style: true,
    })
    .ok()
}

struct FixedClock(u64);

impl Clock for FixedClock {
    fn now_millis(&self) -> u64 {
        self.0
    }
}

#[derive(Default)]
struct Ids(AtomicUsize);

impl IdGenerator for Ids {
    fn image_id(&self) -> Result<ImageId, PortError> {
        let value = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        ImageId::new(format!("boundary-image-{value}")).map_err(|_| PortError::Internal)
    }

    fn object_key(&self, image_id: &ImageId) -> Result<String, PortError> {
        Ok(format!("boundary/{}.png", image_id.as_str()))
    }

    fn event_id(&self) -> Result<String, PortError> {
        Ok("00000000-0000-7000-8000-000000000701".to_owned())
    }
}

struct AllowAuthorization;

#[async_trait]
impl Authorization for AllowAuthorization {
    async fn check(&self, _request: PermissionCheck) -> Result<PermissionDecision, PortError> {
        Ok(PermissionDecision {
            allowed: true,
            checked_at_token: "boundary-check-revision".to_owned(),
        })
    }

    async fn check_service(
        &self,
        _request: ServicePermissionCheck,
    ) -> Result<PermissionDecision, PortError> {
        unreachable!()
    }

    async fn touch_lot_owner(
        &self,
        _request: LotOwnerGrant,
    ) -> Result<AuthorizationRevision, PortError> {
        Ok(AuthorizationRevision {
            revision: "boundary-grant-revision".to_owned(),
        })
    }
}

struct NoopTelemetry;

impl MediaTelemetry for NoopTelemetry {
    fn record(&self, _observation: MediaObservation) {}
}

fn context(key: &str, now: u64) -> CommandContext {
    CommandContext {
        caller_scope: "seller:boundary".to_owned(),
        principal: Some(gavel_catalog::application::Principal {
            principal_id: "seller-boundary".to_owned(),
        }),
        idempotency_key: key.to_owned(),
        correlation_id: format!("correlation-{key}-{now}"),
        consistency_token: None,
    }
}

async fn upload(
    service: &RequestLotImageUpload<
        '_,
        PostgresCatalogStore,
        S3ObjectStorage,
        FixedClock,
        Ids,
        NoopTelemetry,
    >,
    key: &str,
    body: &[u8],
    now: u64,
) -> gavel_catalog::application::UploadIntentResult {
    let checksum = STANDARD.encode(Sha256::digest(body));
    let intent = service
        .execute(RequestUploadInput {
            context: context(key, now),
            content_type: "image/png".to_owned(),
            content_length: body.len() as u64,
            checksum_sha256: checksum,
        })
        .await
        .unwrap();
    let mut request = Client::new().put(&intent.upload_url).body(body.to_vec());
    for (name, value) in &intent.required_headers {
        request = request.header(name, value);
    }
    let response = request.send().await.unwrap();
    assert!(
        response.status().is_success(),
        "upload failed: {}",
        response.status()
    );
    intent
}

#[tokio::test]
async fn real_boundaries_upload_complete_list_query_and_cleanup() {
    let (Some(pool), Some(storage)) = (pool().await, storage()) else {
        eprintln!("Catalog boundary environment is unset; skipping combined integration test");
        return;
    };
    storage.ensure_bucket(Duration::from_secs(2)).await.unwrap();
    let now = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let clock = FixedClock(now);
    let ids = Ids::default();
    let store = PostgresCatalogStore::new(pool.clone());
    let telemetry = NoopTelemetry;
    let request_upload =
        RequestLotImageUpload::new(&store, &storage, &clock, &ids, &telemetry, 120_000, 2_000);
    let attached = upload(&request_upload, "upload-attached", b"attached-image", now).await;
    let orphan = upload(&request_upload, "upload-orphan", b"orphan-image", now).await;
    let complete = CompleteLotImageUpload::new(&store, &storage, &telemetry, 2_000);
    for (key, image_id) in [
        ("complete-attached", &attached.image_id),
        ("complete-orphan", &orphan.image_id),
    ] {
        complete
            .execute(CompleteUploadInput {
                context: context(key, now),
                image_id: image_id.clone(),
                now_millis: now,
            })
            .await
            .unwrap();
    }

    let list = ListLot::new(
        &store,
        b"boundary-hmac-key-at-least-32-bytes",
        &AllowAuthorization,
        &store,
        &ids,
        500,
    );
    list.execute(ListLotInput {
        context: context("list-boundary", now),
        lot_id: "boundary-lot-1".to_owned(),
        title: "Boundary lot".to_owned(),
        description: Some("Real PostgreSQL and s3s-fs".to_owned()),
        images: vec![ImageAttachmentInput {
            image_id: attached.image_id.clone(),
            alt: "attached image".to_owned(),
        }],
        start_price: MoneyInput {
            minor: 100,
            currency_code: "USD".to_owned(),
        },
        bid_step_minor: 10,
        closing_policy: ClosingPolicyInput {
            scheduled_end_millis: now + 60_000,
            inactivity_millis: Some(10_000),
            hard_deadline_millis: Some(now + 120_000),
        },
        occurred_at_millis: now,
    })
    .await
    .unwrap();

    let reader = PostgresPublicReader::new(
        pool.clone(),
        b"boundary-cursor-key-at-least-32-bytes",
        "https://images.example.test",
    );
    let page = reader
        .lots(LotQuery {
            first: 10,
            after: None,
        })
        .await
        .unwrap();
    assert_eq!(page.lots.len(), 1);
    assert_eq!(page.lots[0].lot_id, "boundary-lot-1");
    assert_eq!(page.lots[0].images[0].image_id, attached.image_id);
    assert!(!page.lots[0].images[0].url.contains("boundary/"));

    let leases = CatalogOutbox::lease(&store, "cross-context", now + 1, 10_000, 10)
        .await
        .unwrap();
    assert_eq!(leases.len(), 1);
    let payload = &leases[0].payload;
    let public_json = String::from_utf8(payload.clone()).unwrap();
    for forbidden in [
        "objectKey",
        "authzRevision",
        "consistencyToken",
        "credential",
    ] {
        assert!(!public_json.contains(forbidden));
    }
    let auction_store = std::sync::Arc::new(InMemoryAuctionStore::default());
    let consumer = CatalogEventConsumer::new(
        std::sync::Arc::clone(&auction_store),
        std::sync::Arc::new(InMemoryInbox::default()),
        3,
        [7; 32],
    );
    assert_eq!(
        consumer.handle(payload).await.unwrap(),
        ConsumeOutcome::Applied
    );
    assert_eq!(
        consumer.handle(payload).await.unwrap(),
        ConsumeOutcome::Duplicate
    );
    let loaded = auction_store.load("boundary-lot-1").await.unwrap().unwrap();
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.events.len(), 1);
    let AuctionEvent::Opened {
        auction_id,
        lot_id,
        seller_id,
        start_price,
        bid_step_minor,
        closing_policy,
        ..
    } = &loaded.events[0]
    else {
        panic!("LotListed must open an auction");
    };
    assert_eq!(auction_id.as_str(), "boundary-lot-1");
    assert_eq!(lot_id.as_str(), "boundary-lot-1");
    assert_eq!(seller_id.as_str(), "seller-boundary");
    assert_eq!(start_price.amount_minor(), 100);
    assert_eq!(*bid_step_minor, 10);
    assert_eq!(
        closing_policy.scheduled_end().unwrap().as_millis(),
        now + 60_000
    );

    let cleanup = CleanupWorker::new(&store, &storage, &telemetry, 2_000);
    assert_eq!(
        cleanup
            .run_batch("boundary-worker", now + 60_000, 1, 10_000, 10)
            .await
            .unwrap(),
        1
    );
    let states: Vec<(String, String)> =
        sqlx::query_as("SELECT image_id, state FROM catalog_lot_images ORDER BY image_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(states, vec![(attached.image_id, "ATTACHED".to_owned())]);
}
