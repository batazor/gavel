use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use sqlx::{PgPool, Row};

use crate::{
    application::{ApplicationError, CatalogReader, LotPage, LotQuery, PublicLot, PublicLotImage},
    domain::LotId,
};

type HmacSha256 = Hmac<Sha256>;
const MAX_PAGE_SIZE: usize = 100;

#[derive(Clone)]
pub struct PostgresPublicReader {
    pool: PgPool,
    cursor_key: Vec<u8>,
    delivery_base_url: String,
}

impl PostgresPublicReader {
    pub fn new(
        pool: PgPool,
        cursor_key: impl Into<Vec<u8>>,
        delivery_base_url: impl Into<String>,
    ) -> Self {
        Self {
            pool,
            cursor_key: cursor_key.into(),
            delivery_base_url: delivery_base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    async fn images(&self, lot_id: &str) -> Result<Vec<PublicLotImage>, ApplicationError> {
        let rows = sqlx::query(
            "SELECT image_id, alt_text FROM catalog_lot_images \
             WHERE lot_id = $1 AND state = 'ATTACHED' ORDER BY position ASC",
        )
        .bind(lot_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sql)?;
        rows.into_iter()
            .map(|row| {
                let image_id: String = row
                    .try_get("image_id")
                    .map_err(|_| ApplicationError::Internal)?;
                let alt: String = row
                    .try_get("alt_text")
                    .map_err(|_| ApplicationError::Internal)?;
                Ok(PublicLotImage {
                    url: format!("{}/{}", self.delivery_base_url, image_id),
                    image_id,
                    alt,
                })
            })
            .collect()
    }

    fn encode_cursor(
        &self,
        listed_at_millis: i64,
        lot_id: &str,
    ) -> Result<String, ApplicationError> {
        let payload = CursorPayload {
            version: 1,
            listed_at_millis,
            lot_id: lot_id.to_owned(),
        };
        let bytes = serde_json::to_vec(&payload).map_err(|_| ApplicationError::Internal)?;
        let mut mac =
            HmacSha256::new_from_slice(&self.cursor_key).map_err(|_| ApplicationError::Internal)?;
        mac.update(&bytes);
        let signature = mac.finalize().into_bytes();
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(bytes),
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    fn decode_cursor(&self, cursor: &str) -> Result<CursorPayload, ApplicationError> {
        let (payload, signature) = cursor.split_once('.').ok_or_else(invalid_cursor)?;
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| invalid_cursor())?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| invalid_cursor())?;
        let mut mac =
            HmacSha256::new_from_slice(&self.cursor_key).map_err(|_| ApplicationError::Internal)?;
        mac.update(&payload);
        mac.verify_slice(&signature).map_err(|_| invalid_cursor())?;
        let payload: CursorPayload =
            serde_json::from_slice(&payload).map_err(|_| invalid_cursor())?;
        if payload.version != 1 || payload.listed_at_millis < 0 {
            return Err(invalid_cursor());
        }
        LotId::new(payload.lot_id.clone()).map_err(|_| invalid_cursor())?;
        Ok(payload)
    }
}

#[async_trait::async_trait]
impl CatalogReader for PostgresPublicReader {
    async fn lot(&self, lot_id: &LotId) -> Result<Option<PublicLot>, ApplicationError> {
        let row = sqlx::query(
            "SELECT lot_id, title, description, version FROM catalog_lots \
             WHERE lot_id = $1 AND state = 'LISTED'",
        )
        .bind(lot_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sql)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let lot_id: String = row
            .try_get("lot_id")
            .map_err(|_| ApplicationError::Internal)?;
        Ok(Some(PublicLot {
            images: self.images(&lot_id).await?,
            lot_id,
            title: row
                .try_get("title")
                .map_err(|_| ApplicationError::Internal)?,
            description: row
                .try_get("description")
                .map_err(|_| ApplicationError::Internal)?,
            version: u64::try_from(
                row.try_get::<i64, _>("version")
                    .map_err(|_| ApplicationError::Internal)?,
            )
            .map_err(|_| ApplicationError::Internal)?,
        }))
    }

    async fn lots(&self, query: LotQuery) -> Result<LotPage, ApplicationError> {
        if query.first == 0 || query.first > MAX_PAGE_SIZE {
            return Err(ApplicationError::InvalidArgument {
                reason: "invalid_page_size",
                safe_message: "page size is invalid".to_owned(),
            });
        }
        let after = query
            .after
            .as_deref()
            .map(|cursor| self.decode_cursor(cursor))
            .transpose()?;
        let limit = i64::try_from(query.first + 1).map_err(|_| ApplicationError::Internal)?;
        let rows = if let Some(after) = after {
            sqlx::query(
                "SELECT lot_id, title, description, version, \
                        (extract(epoch FROM listed_at) * 1000)::bigint AS listed_at_millis \
                 FROM catalog_lots WHERE state = 'LISTED' \
                   AND (listed_at, lot_id) > (to_timestamp($1::double precision / 1000.0), $2) \
                 ORDER BY listed_at ASC, lot_id ASC LIMIT $3",
            )
            .bind(after.listed_at_millis)
            .bind(after.lot_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(map_sql)?
        } else {
            sqlx::query(
                "SELECT lot_id, title, description, version, \
                        (extract(epoch FROM listed_at) * 1000)::bigint AS listed_at_millis \
                 FROM catalog_lots WHERE state = 'LISTED' \
                 ORDER BY listed_at ASC, lot_id ASC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(map_sql)?
        };
        let has_next_page = rows.len() > query.first;
        let visible = rows.into_iter().take(query.first).collect::<Vec<_>>();
        let cursors = visible
            .iter()
            .map(|row| {
                self.encode_cursor(
                    row.get::<i64, _>("listed_at_millis"),
                    &row.get::<String, _>("lot_id"),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let end_cursor = cursors.last().cloned();
        let mut lots = Vec::with_capacity(visible.len());
        for row in visible {
            let lot_id: String = row
                .try_get("lot_id")
                .map_err(|_| ApplicationError::Internal)?;
            lots.push(PublicLot {
                images: self.images(&lot_id).await?,
                lot_id,
                title: row
                    .try_get("title")
                    .map_err(|_| ApplicationError::Internal)?,
                description: row
                    .try_get("description")
                    .map_err(|_| ApplicationError::Internal)?,
                version: u64::try_from(
                    row.try_get::<i64, _>("version")
                        .map_err(|_| ApplicationError::Internal)?,
                )
                .map_err(|_| ApplicationError::Internal)?,
            });
        }
        Ok(LotPage {
            lots,
            cursors,
            end_cursor,
            has_next_page,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CursorPayload {
    version: u8,
    listed_at_millis: i64,
    lot_id: String,
}

fn invalid_cursor() -> ApplicationError {
    ApplicationError::InvalidArgument {
        reason: "invalid_cursor",
        safe_message: "cursor is invalid".to_owned(),
    }
}

fn map_sql(error: sqlx::Error) -> ApplicationError {
    match error {
        sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) | sqlx::Error::Tls(_) => {
            ApplicationError::DependencyUnavailable {
                dependency: "catalog_database",
            }
        }
        _ => ApplicationError::Internal,
    }
}
