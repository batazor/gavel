use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use gavel_catalog::{
    application::{
        Authorization, AuthorizationRevision, ClosingPolicyInput, CommandContext, IdGenerator,
        ImageAttachmentInput, ListLot, LotOwnerGrant, MoneyInput, PermissionCheck,
        PermissionDecision, PortError, ServicePermissionCheck,
    },
    domain::ImageId,
};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};

use gavel_catalog::adapters::outbound::postgres::PostgresCatalogStore;

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
        "TRUNCATE catalog_outbox, catalog_reconciliation_queue, catalog_inbox, \
         catalog_command_receipts, catalog_lot_images, catalog_lots CASCADE",
    )
    .execute(&pool)
    .await
    .unwrap();
    Some(pool)
}

struct AllowAuthorization;

#[async_trait]
impl Authorization for AllowAuthorization {
    async fn check(&self, _request: PermissionCheck) -> Result<PermissionDecision, PortError> {
        Ok(PermissionDecision {
            allowed: true,
            checked_at_token: "check-revision".to_owned(),
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
            revision: "grant-revision".to_owned(),
        })
    }
}

struct Ids {
    event_id: &'static str,
}

impl IdGenerator for Ids {
    fn image_id(&self) -> Result<ImageId, PortError> {
        unreachable!()
    }

    fn object_key(&self, _image_id: &ImageId) -> Result<String, PortError> {
        unreachable!()
    }

    fn event_id(&self) -> Result<String, PortError> {
        Ok(self.event_id.to_owned())
    }
}

fn input(key: &str) -> gavel_catalog::application::ListLotInput {
    gavel_catalog::application::ListLotInput {
        context: CommandContext {
            caller_scope: "seller-1".to_owned(),
            principal: Some(gavel_catalog::application::Principal {
                principal_id: "seller-1".to_owned(),
            }),
            idempotency_key: key.to_owned(),
            correlation_id: "correlation-1".to_owned(),
            consistency_token: None,
        },
        lot_id: "lot-1".to_owned(),
        title: "Vintage clock".to_owned(),
        description: Some("Works".to_owned()),
        images: vec![ImageAttachmentInput {
            image_id: "image-1".to_owned(),
            alt: "front".to_owned(),
        }],
        start_price: MoneyInput {
            minor: 100,
            currency_code: "USD".to_owned(),
        },
        bid_step_minor: 10,
        closing_policy: ClosingPolicyInput {
            scheduled_end_millis: 2_000,
            inactivity_millis: Some(100),
            hard_deadline_millis: Some(3_000),
        },
        occurred_at_millis: 1_000,
    }
}

async fn insert_ready_image(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO catalog_lot_images \
         (image_id, owner_id, object_key, expected_content_type, expected_content_length, \
          expected_checksum_sha256, expires_at, state, ready_at) \
         VALUES ('image-1', 'seller-1', 'private/image-1', 'image/png', 42, $1, \
                 to_timestamp(3), 'READY', to_timestamp(1))",
    )
    .bind(STANDARD.encode([9_u8; 32]))
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn create_is_atomic_and_replay_conflict_and_concurrency_have_one_effect() {
    let Some(pool) = pool().await else {
        return;
    };
    insert_ready_image(&pool).await;
    let store = PostgresCatalogStore::new(pool.clone());
    let ids = Ids {
        event_id: "00000000-0000-7000-8000-000000000101",
    };
    let service = ListLot::new(
        &store,
        b"test-hmac-key-at-least-32-bytes!",
        &AllowAuthorization,
        &store,
        &ids,
        250,
    );
    let result = service.execute(input("key-1")).await.unwrap();
    assert_eq!(result.version, 1);
    assert_eq!(service.execute(input("key-1")).await.unwrap(), result);
    let mut changed = input("key-1");
    changed.title = "Changed".to_owned();
    assert!(matches!(
        service.execute(changed).await,
        Err(gavel_catalog::application::ApplicationError::IdempotencyConflict)
    ));

    let row = sqlx::query(
        "SELECT l.authz_revision, i.state, i.position, r.status, count(o.event_id) OVER () AS events \
         FROM catalog_lots l JOIN catalog_lot_images i ON i.lot_id = l.lot_id \
         JOIN catalog_command_receipts r ON r.caller_scope = 'seller-1' \
         JOIN catalog_outbox o ON o.aggregate_id = l.lot_id WHERE l.lot_id = 'lot-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("authz_revision"), "grant-revision");
    assert_eq!(row.get::<String, _>("state"), "ATTACHED");
    assert_eq!(row.get::<i16, _>("position"), 0);
    assert_eq!(row.get::<String, _>("status"), "COMPLETED");
    assert_eq!(row.get::<i64, _>("events"), 1);

    let (left, right) = tokio::join!(
        service.execute(input("key-concurrent-a")),
        service.execute(input("key-concurrent-b"))
    );
    assert!(left.is_ok());
    assert!(right.is_ok());
    let outbox_count: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog_outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(outbox_count, 1);
}

#[tokio::test]
async fn outbox_failure_rolls_back_lot_attachment_and_receipt_result() {
    let Some(pool) = pool().await else {
        return;
    };
    insert_ready_image(&pool).await;
    let store = PostgresCatalogStore::new(pool.clone());
    let ids = Ids {
        event_id: "not-a-uuid",
    };
    let service = ListLot::new(
        &store,
        b"test-hmac-key-at-least-32-bytes!",
        &AllowAuthorization,
        &store,
        &ids,
        250,
    );
    assert!(service.execute(input("key-rollback")).await.is_err());
    let lot_count: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog_lots")
        .fetch_one(&pool)
        .await
        .unwrap();
    let outbox_count: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog_outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    let state: String =
        sqlx::query_scalar("SELECT state FROM catalog_lot_images WHERE image_id = 'image-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog_command_receipts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((lot_count, outbox_count, receipts), (0, 0, 0));
    assert_eq!(state, "READY");
}
