use gavel_catalog::{
    adapters::outbound::postgres_reader::PostgresPublicReader,
    application::{ApplicationError, CatalogReader, LotQuery},
    domain::LotId,
};
use sqlx::{PgPool, postgres::PgPoolOptions};

async fn pool() -> Option<PgPool> {
    let url = match std::env::var("CATALOG_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("CATALOG_TEST_DATABASE_URL is unset; skipping PostgreSQL integration test");
            return None;
        }
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("test PostgreSQL must be reachable");
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("TRUNCATE catalog_lot_images, catalog_outbox, catalog_lots CASCADE")
        .execute(&pool)
        .await
        .unwrap();
    Some(pool)
}

async fn insert_lot(pool: &PgPool, id: &str, listed_at_millis: i64, state: &str) {
    sqlx::query(
        "INSERT INTO catalog_lots \
         (lot_id, seller_id, title, description, start_price_minor, currency_code, \
          bid_step_minor, scheduled_end_millis, state, version, authz_revision, listed_at) \
         VALUES ($1, 'seller-1', $2, 'Public', 100, 'USD', 10, 5000, $3, 1, \
                 'revision', to_timestamp($4::double precision / 1000.0))",
    )
    .bind(id)
    .bind(format!("Title {id}"))
    .bind(state)
    .bind(listed_at_millis)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn hmac_keyset_pages_are_stable_across_inserts_and_hide_non_listed_rows() {
    let Some(pool) = pool().await else {
        return;
    };
    insert_lot(&pool, "lot-1", 1_000, "LISTED").await;
    insert_lot(&pool, "lot-2", 2_000, "LISTED").await;
    insert_lot(&pool, "lot-3", 3_000, "LISTED").await;
    insert_lot(&pool, "lot-hidden", 1_500, "WITHDRAWN").await;
    sqlx::query(
        "INSERT INTO catalog_lot_images \
         (image_id, owner_id, object_key, expected_content_type, expected_content_length, \
          expected_checksum_sha256, expires_at, state, ready_at, lot_id, position, alt_text) \
         VALUES ('image-1', 'seller-1', 'private/secret-object', 'image/png', 42, 'checksum', \
                 to_timestamp(4), 'ATTACHED', to_timestamp(1), 'lot-1', 0, 'front')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let reader = PostgresPublicReader::new(
        pool.clone(),
        b"cursor-hmac-key-at-least-32-bytes!",
        "https://media.example.test/catalog/",
    );

    let first = reader
        .lots(LotQuery {
            first: 1,
            after: None,
        })
        .await
        .unwrap();
    assert_eq!(first.lots[0].lot_id, "lot-1");
    assert!(first.has_next_page);
    assert_eq!(
        first.lots[0].images[0].url,
        "https://media.example.test/catalog/image-1"
    );
    assert!(!first.lots[0].images[0].url.contains("secret-object"));

    insert_lot(&pool, "lot-between", 1_500, "LISTED").await;
    let second = reader
        .lots(LotQuery {
            first: 10,
            after: first.end_cursor.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        second
            .lots
            .iter()
            .map(|lot| lot.lot_id.as_str())
            .collect::<Vec<_>>(),
        ["lot-between", "lot-2", "lot-3"]
    );
    assert!(!second.has_next_page);
    assert!(
        reader
            .lot(&LotId::new("lot-hidden").unwrap())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn malformed_tampered_cursor_and_page_limits_are_rejected() {
    let Some(pool) = pool().await else {
        return;
    };
    insert_lot(&pool, "lot-1", 1_000, "LISTED").await;
    let reader = PostgresPublicReader::new(
        pool,
        b"cursor-hmac-key-at-least-32-bytes!",
        "https://media.example.test",
    );
    let page = reader
        .lots(LotQuery {
            first: 1,
            after: None,
        })
        .await
        .unwrap();
    let mut tampered = page.end_cursor.unwrap();
    tampered.push('A');
    for after in [Some("malformed".to_owned()), Some(tampered)] {
        let error = reader.lots(LotQuery { first: 1, after }).await.unwrap_err();
        assert_eq!(error.reason(), "invalid_cursor");
    }
    for first in [0, 101] {
        assert!(matches!(
            reader.lots(LotQuery { first, after: None }).await,
            Err(ApplicationError::InvalidArgument { .. })
        ));
    }
}
