use std::str::FromStr;

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    adapters::outbound::events::{
        ClosingPolicyV1, EventEnvelope, LotListedV1, LotUpdatedV1, LotWithdrawnV1, encode,
    },
    application::{
        AuctionEventInbox, CatalogCommitError, CatalogEvent, CatalogOutbox, CatalogUnitOfWork,
        CleanupLease, CommandReceiptClaim, CommandReceiptOutcome, CommandReceiptStore,
        FreezeFromAuction, FreezeInboxOutcome, GrantReconciliationLease, GrantReconciliationQueue,
        ListLotCommit, LotCommandResult, LotMutationResult, MediaRepository, OutboxLease,
        PortError, ReceiptCompletion, UpdateLotCommit, UploadClaim, UploadClaimOutcome,
        UploadIntentResult, WithdrawLotCommit,
    },
    domain::{
        AltText, DomainError, ImageId, ImageState, LotId, LotImage, ObjectMetadata, PrincipalId,
    },
};

#[derive(Clone)]
pub struct PostgresCatalogStore {
    pool: PgPool,
}

impl PostgresCatalogStore {
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }
}

#[async_trait]
impl CommandReceiptStore for PostgresCatalogStore {
    async fn claim(&self, claim: CommandReceiptClaim) -> Result<CommandReceiptOutcome, PortError> {
        let fingerprint = URL_SAFE_NO_PAD
            .decode(claim.fingerprint)
            .map_err(|_| PortError::Internal)?;
        if fingerprint.len() != 32 {
            return Err(PortError::Internal);
        }
        let token = Uuid::now_v7();
        let inserted = sqlx::query(
            "INSERT INTO catalog_command_receipts \
             (caller_scope, idempotency_key, command, request_fingerprint, status, claim_token) \
             VALUES ($1, $2, $3, $4, 'CLAIMED', $5) ON CONFLICT DO NOTHING",
        )
        .bind(&claim.caller_scope)
        .bind(&claim.idempotency_key)
        .bind(claim.command)
        .bind(&fingerprint)
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        if inserted.rows_affected() == 1 {
            return Ok(CommandReceiptOutcome::Acquired {
                claim_token: token.to_string(),
            });
        }
        let row = sqlx::query(
            "SELECT command, request_fingerprint, status, result_payload \
             FROM catalog_command_receipts WHERE caller_scope = $1 AND idempotency_key = $2",
        )
        .bind(&claim.caller_scope)
        .bind(&claim.idempotency_key)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sql)?;
        let stored_command: String = row.try_get("command").map_err(|_| PortError::Internal)?;
        let stored_fingerprint: Vec<u8> = row
            .try_get("request_fingerprint")
            .map_err(|_| PortError::Internal)?;
        if stored_command != claim.command || stored_fingerprint != fingerprint {
            return Ok(CommandReceiptOutcome::Conflict);
        }
        let status: String = row.try_get("status").map_err(|_| PortError::Internal)?;
        if status == "COMPLETED" {
            let result: serde_json::Value = row
                .try_get("result_payload")
                .map_err(|_| PortError::Internal)?;
            return Ok(CommandReceiptOutcome::Replay {
                result_json: serde_json::to_vec(&result).map_err(|_| PortError::Internal)?,
            });
        }
        Ok(CommandReceiptOutcome::InProgress)
    }

    async fn complete(
        &self,
        claim_token: &str,
        result_json: &[u8],
    ) -> Result<ReceiptCompletion, PortError> {
        let token = Uuid::from_str(claim_token).map_err(|_| PortError::Internal)?;
        let result: serde_json::Value =
            serde_json::from_slice(result_json).map_err(|_| PortError::Internal)?;
        let updated = sqlx::query(
            "UPDATE catalog_command_receipts SET status = 'COMPLETED', result_payload = $2, \
             updated_at = clock_timestamp() WHERE claim_token = $1 AND status = 'CLAIMED'",
        )
        .bind(token)
        .bind(&result)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        if updated.rows_affected() == 1 {
            return Ok(ReceiptCompletion::Stored);
        }
        let replay: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT result_payload FROM catalog_command_receipts \
             WHERE claim_token = $1 AND status = 'COMPLETED'",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sql)?
        .flatten();
        Ok(if replay.as_ref() == Some(&result) {
            ReceiptCompletion::Stored
        } else {
            ReceiptCompletion::LostClaim
        })
    }

    async fn release_retry(&self, claim_token: &str) -> Result<(), PortError> {
        let token = Uuid::from_str(claim_token).map_err(|_| PortError::Internal)?;
        sqlx::query(
            "DELETE FROM catalog_command_receipts WHERE claim_token = $1 AND status = 'CLAIMED'",
        )
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        Ok(())
    }
}

#[async_trait]
impl CatalogUnitOfWork for PostgresCatalogStore {
    async fn create_lot(
        &self,
        commit: ListLotCommit,
    ) -> Result<LotCommandResult, CatalogCommitError> {
        let mut transaction = self.pool.begin().await.map_err(map_commit_sql)?;
        if let Some(row) = sqlx::query(
            "SELECT version, seller_id, title FROM catalog_lots WHERE lot_id = $1 FOR UPDATE",
        )
        .bind(commit.lot.lot_id().as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_commit_sql)?
        {
            let version: i64 = row.try_get("version").map_err(|_| internal_commit())?;
            let seller: String = row.try_get("seller_id").map_err(|_| internal_commit())?;
            let title: String = row.try_get("title").map_err(|_| internal_commit())?;
            if seller != commit.lot.seller().as_str() || title != commit.lot.title().as_str() {
                return Err(CatalogCommitError::Conflict);
            }
            let result = LotCommandResult {
                lot_id: commit.lot.lot_id().to_string(),
                version: to_u64(version)?,
            };
            complete_receipt(&mut transaction, &commit.receipt, &result).await?;
            transaction.commit().await.map_err(map_commit_sql)?;
            return Ok(result);
        }

        let requested_ids: Vec<_> = commit
            .attachments
            .iter()
            .map(|attachment| attachment.image_id.as_str())
            .collect();
        let rows = sqlx::query(
            "SELECT image_id, owner_id, state FROM catalog_lot_images \
             WHERE image_id = ANY($1) FOR UPDATE",
        )
        .bind(&requested_ids)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_commit_sql)?;
        if rows.len() != commit.attachments.len() {
            return Err(CatalogCommitError::Domain(DomainError::InvalidImageBatch));
        }
        for attachment in &commit.attachments {
            let row = rows
                .iter()
                .find(|row| row.get::<String, _>("image_id") == attachment.image_id.as_str())
                .ok_or(CatalogCommitError::Domain(DomainError::InvalidImageBatch))?;
            let owner: String = row.try_get("owner_id").map_err(|_| internal_commit())?;
            let state: String = row.try_get("state").map_err(|_| internal_commit())?;
            if owner != commit.lot.seller().as_str() || state != "READY" {
                return Err(CatalogCommitError::Domain(DomainError::InvalidImageBatch));
            }
        }

        sqlx::query(
            "INSERT INTO catalog_lots \
             (lot_id, seller_id, title, description, start_price_minor, currency_code, \
              bid_step_minor, scheduled_end_millis, inactivity_millis, hard_deadline_millis, \
              state, version, authz_revision, description_frozen, auction_source_version, listed_at) \
             VALUES ($1, $2, $3, $4, $5::numeric, $6, $7::numeric, $8, $9, $10, \
                     'LISTED', 1, $11, FALSE, 0, $12)",
        )
        .bind(commit.lot.lot_id().as_str())
        .bind(commit.lot.seller().as_str())
        .bind(commit.lot.title().as_str())
        .bind(commit.lot.description().as_deref())
        .bind(commit.lot.start_price().minor().to_string())
        .bind(commit.lot.start_price().currency().as_str())
        .bind(commit.lot.bid_step().minor().to_string())
        .bind(to_i64(commit.lot.closing_policy().scheduled_end_millis())?)
        .bind(optional_i64(commit.lot.closing_policy().inactivity_millis())?)
        .bind(optional_i64(commit.lot.closing_policy().hard_deadline_millis())?)
        .bind(commit.lot.authz_revision())
        .bind(millis_to_datetime(commit.lot.listed_at_millis())?)
        .execute(&mut *transaction)
        .await
        .map_err(map_commit_sql)?;
        for (position, attachment) in commit.attachments.iter().enumerate() {
            let updated = sqlx::query(
                "UPDATE catalog_lot_images SET state = 'ATTACHED', lot_id = $2, position = $3, \
                 alt_text = $4, updated_at = clock_timestamp() \
                 WHERE image_id = $1 AND owner_id = $5 AND state = 'READY'",
            )
            .bind(attachment.image_id.as_str())
            .bind(commit.lot.lot_id().as_str())
            .bind(i16::try_from(position).map_err(|_| internal_commit())?)
            .bind(attachment.alt.as_str())
            .bind(commit.lot.seller().as_str())
            .execute(&mut *transaction)
            .await
            .map_err(map_commit_sql)?;
            if updated.rows_affected() != 1 {
                return Err(CatalogCommitError::Conflict);
            }
        }
        insert_listed_outbox(&mut transaction, &commit).await?;
        complete_receipt(&mut transaction, &commit.receipt, &commit.result).await?;
        transaction.commit().await.map_err(map_commit_sql)?;
        Ok(commit.result)
    }

    async fn update_lot(
        &self,
        commit: UpdateLotCommit,
    ) -> Result<LotCommandResult, CatalogCommitError> {
        let mut transaction = self.pool.begin().await.map_err(map_commit_sql)?;
        let row = sqlx::query(
            "SELECT seller_id, state, version, description_frozen, title, description \
             FROM catalog_lots WHERE lot_id = $1 FOR UPDATE",
        )
        .bind(commit.lot_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_commit_sql)?
        .ok_or(CatalogCommitError::Port(PortError::NotFound))?;
        let seller: String = row.try_get("seller_id").map_err(|_| internal_commit())?;
        let state: String = row.try_get("state").map_err(|_| internal_commit())?;
        let version: i64 = row.try_get("version").map_err(|_| internal_commit())?;
        let frozen: bool = row
            .try_get("description_frozen")
            .map_err(|_| internal_commit())?;
        if seller != commit.seller.as_str() || state != "LISTED" {
            return Err(CatalogCommitError::Conflict);
        }
        if to_u64(version)? != commit.expected_version {
            return Err(CatalogCommitError::Domain(DomainError::VersionConflict));
        }
        if frozen {
            return Err(CatalogCommitError::Domain(DomainError::DescriptionFrozen));
        }
        let title = commit.title.as_ref().map_or_else(
            || {
                row.try_get::<String, _>("title")
                    .map_err(|_| internal_commit())
            },
            |title| Ok(title.as_str().to_owned()),
        )?;
        let description = match &commit.description {
            Some(description) => description.as_deref().map(str::to_owned),
            None => row
                .try_get::<Option<String>, _>("description")
                .map_err(|_| internal_commit())?,
        };
        let next_version = version.checked_add(1).ok_or_else(internal_commit)?;
        sqlx::query(
            "UPDATE catalog_lots SET title = $2, description = $3, version = $4, \
             updated_at = clock_timestamp() WHERE lot_id = $1",
        )
        .bind(commit.lot_id.as_str())
        .bind(&title)
        .bind(&description)
        .bind(next_version)
        .execute(&mut *transaction)
        .await
        .map_err(map_commit_sql)?;
        insert_updated_outbox(
            &mut transaction,
            &commit,
            to_u64(next_version)?,
            &title,
            description,
        )
        .await?;
        let result = LotCommandResult {
            lot_id: commit.lot_id.to_string(),
            version: to_u64(next_version)?,
        };
        complete_receipt(&mut transaction, &commit.receipt, &result).await?;
        transaction.commit().await.map_err(map_commit_sql)?;
        Ok(result)
    }

    async fn withdraw_lot(
        &self,
        commit: WithdrawLotCommit,
    ) -> Result<LotMutationResult, CatalogCommitError> {
        let mut transaction = self.pool.begin().await.map_err(map_commit_sql)?;
        let row =
            sqlx::query("SELECT state, version FROM catalog_lots WHERE lot_id = $1 FOR UPDATE")
                .bind(commit.lot_id.as_str())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(map_commit_sql)?
                .ok_or(CatalogCommitError::Port(PortError::NotFound))?;
        let state: String = row.try_get("state").map_err(|_| internal_commit())?;
        let version: i64 = row.try_get("version").map_err(|_| internal_commit())?;
        let next_version = if state == "WITHDRAWN" {
            version
        } else if state == "LISTED" {
            let next = version.checked_add(1).ok_or_else(internal_commit)?;
            sqlx::query(
                "UPDATE catalog_lots SET state = 'WITHDRAWN', version = $2, \
                 updated_at = clock_timestamp() WHERE lot_id = $1",
            )
            .bind(commit.lot_id.as_str())
            .bind(next)
            .execute(&mut *transaction)
            .await
            .map_err(map_commit_sql)?;
            insert_withdrawn_outbox(&mut transaction, &commit, to_u64(next)?).await?;
            next
        } else {
            return Err(CatalogCommitError::Conflict);
        };
        let result = LotMutationResult {
            lot_id: commit.lot_id.to_string(),
            version: to_u64(next_version)?,
            checked_at_token: commit.checked_at_token,
        };
        complete_receipt(&mut transaction, &commit.receipt, &result).await?;
        transaction.commit().await.map_err(map_commit_sql)?;
        Ok(result)
    }
}

#[async_trait]
impl AuctionEventInbox for PostgresCatalogStore {
    async fn freeze_from_first_bid(
        &self,
        event: FreezeFromAuction,
    ) -> Result<FreezeInboxOutcome, PortError> {
        let fingerprint = URL_SAFE_NO_PAD
            .decode(event.payload_fingerprint)
            .map_err(|_| PortError::Internal)?;
        if fingerprint.len() != 32 {
            return Err(PortError::Internal);
        }
        let mut transaction = self.pool.begin().await.map_err(map_sql)?;
        if let Some(row) = sqlx::query(
            "SELECT payload_fingerprint, status FROM catalog_inbox \
             WHERE consumer = 'auction.bid-recorded.v1' AND event_id = $1 FOR UPDATE",
        )
        .bind(&event.event_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_sql)?
        {
            let stored: Vec<u8> = row
                .try_get("payload_fingerprint")
                .map_err(|_| PortError::Internal)?;
            if stored == fingerprint {
                transaction.commit().await.map_err(map_sql)?;
                return Ok(FreezeInboxOutcome::Duplicate);
            }
            sqlx::query(
                "UPDATE catalog_inbox SET status = 'QUARANTINED', \
                 processed_at = clock_timestamp() \
                 WHERE consumer = 'auction.bid-recorded.v1' AND event_id = $1",
            )
            .bind(&event.event_id)
            .execute(&mut *transaction)
            .await
            .map_err(map_sql)?;
            transaction.commit().await.map_err(map_sql)?;
            return Ok(FreezeInboxOutcome::Quarantined);
        }
        let current: Option<i64> = sqlx::query_scalar(
            "SELECT auction_source_version FROM catalog_lots WHERE lot_id = $1 FOR UPDATE",
        )
        .bind(event.lot_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_sql)?;
        let current = current.ok_or(PortError::NotFound)?;
        let source_version =
            i64::try_from(event.source_version).map_err(|_| PortError::Internal)?;
        let (status, outcome) = if current >= source_version {
            ("STALE", FreezeInboxOutcome::Stale)
        } else {
            sqlx::query(
                "UPDATE catalog_lots SET description_frozen = TRUE, auction_source_version = $2, \
                 updated_at = clock_timestamp() WHERE lot_id = $1",
            )
            .bind(event.lot_id.as_str())
            .bind(source_version)
            .execute(&mut *transaction)
            .await
            .map_err(map_sql)?;
            ("PROCESSED", FreezeInboxOutcome::Applied)
        };
        sqlx::query(
            "INSERT INTO catalog_inbox \
             (consumer, event_id, payload_fingerprint, aggregate_id, source_version, status) \
             VALUES ('auction.bid-recorded.v1', $1, $2, $3, $4, $5)",
        )
        .bind(&event.event_id)
        .bind(fingerprint)
        .bind(event.lot_id.as_str())
        .bind(source_version)
        .bind(status)
        .execute(&mut *transaction)
        .await
        .map_err(map_sql)?;
        transaction.commit().await.map_err(map_sql)?;
        Ok(outcome)
    }
}

#[async_trait]
impl MediaRepository for PostgresCatalogStore {
    async fn claim_upload(&self, claim: UploadClaim) -> Result<UploadClaimOutcome, PortError> {
        let fingerprint = URL_SAFE_NO_PAD
            .decode(claim.fingerprint)
            .map_err(|_| PortError::Internal)?;
        if fingerprint.len() != 32 {
            return Err(PortError::Internal);
        }
        let mut transaction = self.pool.begin().await.map_err(map_sql)?;
        let token = Uuid::now_v7();
        let inserted = sqlx::query(
            "INSERT INTO catalog_command_receipts \
             (caller_scope, idempotency_key, command, request_fingerprint, status, claim_token) \
             VALUES ($1, $2, 'request_image_upload', $3, 'CLAIMED', $4) \
             ON CONFLICT DO NOTHING",
        )
        .bind(&claim.caller_scope)
        .bind(&claim.idempotency_key)
        .bind(&fingerprint)
        .bind(token)
        .execute(&mut *transaction)
        .await
        .map_err(map_sql)?;
        if inserted.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO catalog_lot_images \
                 (image_id, owner_id, object_key, expected_content_type, expected_content_length, \
                  expected_checksum_sha256, expires_at, state) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 'PENDING')",
            )
            .bind(claim.image.image_id().as_str())
            .bind(claim.image.owner().as_str())
            .bind(claim.image.object_key())
            .bind(claim.image.expected().content_type())
            .bind(
                i64::try_from(claim.image.expected().content_length())
                    .map_err(|_| PortError::Internal)?,
            )
            .bind(claim.image.expected().checksum_sha256())
            .bind(datetime_from_millis_port(claim.image.expires_at_millis())?)
            .execute(&mut *transaction)
            .await
            .map_err(map_sql)?;
            transaction.commit().await.map_err(map_sql)?;
            return Ok(UploadClaimOutcome::Created(claim.image));
        }
        let receipt = sqlx::query(
            "SELECT command, request_fingerprint, status, result_payload \
             FROM catalog_command_receipts WHERE caller_scope = $1 AND idempotency_key = $2",
        )
        .bind(&claim.caller_scope)
        .bind(&claim.idempotency_key)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_sql)?;
        let command: String = receipt
            .try_get("command")
            .map_err(|_| PortError::Internal)?;
        let stored: Vec<u8> = receipt
            .try_get("request_fingerprint")
            .map_err(|_| PortError::Internal)?;
        if command != "request_image_upload" || stored != fingerprint {
            return Ok(UploadClaimOutcome::Conflict);
        }
        let status: String = receipt.try_get("status").map_err(|_| PortError::Internal)?;
        if status == "COMPLETED" {
            let result: serde_json::Value = receipt
                .try_get("result_payload")
                .map_err(|_| PortError::Internal)?;
            let result = serde_json::from_value(result).map_err(|_| PortError::Internal)?;
            transaction.commit().await.map_err(map_sql)?;
            return Ok(UploadClaimOutcome::CompletedReplay(result));
        }
        let row = sqlx::query("SELECT * FROM catalog_lot_images WHERE image_id = $1")
            .bind(claim.image.image_id().as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_sql)?;
        let image = image_from_row(&row)?;
        transaction.commit().await.map_err(map_sql)?;
        Ok(UploadClaimOutcome::PendingReplay(image))
    }

    async fn complete_upload_request(
        &self,
        caller_scope: &str,
        idempotency_key: &str,
        result: &UploadIntentResult,
    ) -> Result<(), PortError> {
        let payload = serde_json::to_value(result).map_err(|_| PortError::Internal)?;
        let updated = sqlx::query(
            "UPDATE catalog_command_receipts SET status = 'COMPLETED', result_payload = $3, \
             updated_at = clock_timestamp() WHERE caller_scope = $1 AND idempotency_key = $2 \
             AND command = 'request_image_upload' AND status = 'CLAIMED'",
        )
        .bind(caller_scope)
        .bind(idempotency_key)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        if updated.rows_affected() != 1 {
            return Err(PortError::Internal);
        }
        Ok(())
    }

    async fn image(&self, image_id: &ImageId) -> Result<Option<LotImage>, PortError> {
        let row = sqlx::query("SELECT * FROM catalog_lot_images WHERE image_id = $1")
            .bind(image_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sql)?;
        row.as_ref().map(image_from_row).transpose()
    }

    async fn save_verified(&self, image: &LotImage) -> Result<(), PortError> {
        let ImageState::Ready { ready_at_millis } = image.state() else {
            return Err(PortError::Internal);
        };
        let updated = sqlx::query(
            "UPDATE catalog_lot_images SET state = 'READY', ready_at = $2, \
             updated_at = clock_timestamp() WHERE image_id = $1 AND state = 'PENDING'",
        )
        .bind(image.image_id().as_str())
        .bind(datetime_from_millis_port(*ready_at_millis)?)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        if updated.rows_affected() != 1 {
            return Err(PortError::Internal);
        }
        Ok(())
    }

    async fn mark_for_cleanup(&self, image: &LotImage) -> Result<(), PortError> {
        if !matches!(image.state(), ImageState::Invalid) {
            return Err(PortError::Internal);
        }
        let updated = sqlx::query(
            "UPDATE catalog_lot_images SET state = 'INVALID', updated_at = clock_timestamp() \
             WHERE image_id = $1 AND state <> 'ATTACHED'",
        )
        .bind(image.image_id().as_str())
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        if updated.rows_affected() != 1 {
            return Err(PortError::Internal);
        }
        Ok(())
    }

    async fn lease_cleanup(
        &self,
        worker: &str,
        now_millis: u64,
        ready_ttl_millis: u64,
        lease_millis: u64,
        limit: usize,
    ) -> Result<Vec<CleanupLease>, PortError> {
        let now = datetime_from_millis_port(now_millis)?;
        let ready_cutoff = datetime_from_millis_port(now_millis.saturating_sub(ready_ttl_millis))?;
        let lease_until = datetime_from_millis_port(now_millis.saturating_add(lease_millis))?;
        let limit = i64::try_from(limit).map_err(|_| PortError::Internal)?;
        let token = Uuid::now_v7();
        let rows = sqlx::query(
            "WITH candidates AS ( \
               SELECT image_id FROM catalog_lot_images \
               WHERE state <> 'ATTACHED' \
                 AND (cleanup_lease_until IS NULL OR cleanup_lease_until <= $1) \
                 AND (state = 'INVALID' OR (state = 'PENDING' AND expires_at < $1) \
                      OR (state = 'READY' AND ready_at < $2)) \
               ORDER BY created_at, image_id FOR UPDATE SKIP LOCKED LIMIT $3 \
             ) UPDATE catalog_lot_images image \
               SET cleanup_lease_owner = $4, cleanup_lease_token = $5, cleanup_lease_until = $6 \
             FROM candidates WHERE image.image_id = candidates.image_id RETURNING image.*",
        )
        .bind(now)
        .bind(ready_cutoff)
        .bind(limit)
        .bind(worker)
        .bind(token)
        .bind(lease_until)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sql)?;
        rows.iter()
            .map(|row| {
                Ok(CleanupLease {
                    image: image_from_row(row)?,
                    token: token.to_string(),
                })
            })
            .collect()
    }

    async fn complete_cleanup(&self, image_id: &ImageId, token: &str) -> Result<(), PortError> {
        let token = Uuid::from_str(token).map_err(|_| PortError::Internal)?;
        sqlx::query(
            "DELETE FROM catalog_lot_images WHERE image_id = $1 AND cleanup_lease_token = $2 \
             AND state <> 'ATTACHED'",
        )
        .bind(image_id.as_str())
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        Ok(())
    }

    async fn release_cleanup(&self, image_id: &ImageId, token: &str) -> Result<(), PortError> {
        let token = Uuid::from_str(token).map_err(|_| PortError::Internal)?;
        sqlx::query(
            "UPDATE catalog_lot_images SET cleanup_lease_owner = NULL, cleanup_lease_token = NULL, \
             cleanup_lease_until = NULL WHERE image_id = $1 AND cleanup_lease_token = $2",
        )
        .bind(image_id.as_str())
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        Ok(())
    }
}

#[async_trait]
impl GrantReconciliationQueue for PostgresCatalogStore {
    async fn lease_due(
        &self,
        worker: &str,
        now_millis: u64,
        lease_millis: u64,
        limit: usize,
    ) -> Result<Vec<GrantReconciliationLease>, PortError> {
        let now = datetime_from_millis_port(now_millis)?;
        let until = datetime_from_millis_port(now_millis.saturating_add(lease_millis))?;
        let token = Uuid::now_v7();
        let rows = sqlx::query(
            "WITH candidates AS ( \
               SELECT operation_id FROM catalog_reconciliation_queue \
               WHERE operation_type = 'AUTHZ_GRANT_DELETE' AND completed_at IS NULL \
                 AND not_before <= $1 AND (lease_until IS NULL OR lease_until <= $1) \
               ORDER BY not_before, created_at FOR UPDATE SKIP LOCKED LIMIT $2 \
             ) UPDATE catalog_reconciliation_queue queue \
               SET lease_owner = $3, lease_token = $4, lease_until = $5, attempts = attempts + 1 \
             FROM candidates WHERE queue.operation_id = candidates.operation_id \
             RETURNING queue.operation_id, queue.resource_id, queue.owner_id, \
                       queue.idempotency_key, queue.not_before",
        )
        .bind(now)
        .bind(i64::try_from(limit).map_err(|_| PortError::Internal)?)
        .bind(worker)
        .bind(token)
        .bind(until)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sql)?;
        rows.into_iter()
            .map(|row| {
                let not_before: DateTime<Utc> =
                    row.try_get("not_before").map_err(|_| PortError::Internal)?;
                Ok(GrantReconciliationLease {
                    operation_id: row
                        .try_get("operation_id")
                        .map_err(|_| PortError::Internal)?,
                    lot_id: LotId::new(
                        row.try_get::<String, _>("resource_id")
                            .map_err(|_| PortError::Internal)?,
                    )
                    .map_err(|_| PortError::Internal)?,
                    seller: PrincipalId::new(
                        row.try_get::<String, _>("owner_id")
                            .map_err(|_| PortError::Internal)?,
                    )
                    .map_err(|_| PortError::Internal)?,
                    delete_idempotency_key: row
                        .try_get("idempotency_key")
                        .map_err(|_| PortError::Internal)?,
                    not_before_millis: u64::try_from(not_before.timestamp_millis())
                        .map_err(|_| PortError::Internal)?,
                    lease_token: token.to_string(),
                })
            })
            .collect()
    }

    async fn lot_exists(&self, lot_id: &LotId) -> Result<bool, PortError> {
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM catalog_lots WHERE lot_id = $1)")
            .bind(lot_id.as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(map_sql)
    }

    async fn complete(&self, operation_id: &str, lease_token: &str) -> Result<(), PortError> {
        let token = Uuid::from_str(lease_token).map_err(|_| PortError::Internal)?;
        sqlx::query(
            "UPDATE catalog_reconciliation_queue SET completed_at = clock_timestamp(), \
             lease_owner = NULL, lease_token = NULL, lease_until = NULL \
             WHERE operation_id = $1 AND lease_token = $2",
        )
        .bind(operation_id)
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        Ok(())
    }

    async fn release(&self, operation_id: &str, lease_token: &str) -> Result<(), PortError> {
        let token = Uuid::from_str(lease_token).map_err(|_| PortError::Internal)?;
        sqlx::query(
            "UPDATE catalog_reconciliation_queue SET lease_owner = NULL, lease_token = NULL, \
             lease_until = NULL WHERE operation_id = $1 AND lease_token = $2",
        )
        .bind(operation_id)
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        Ok(())
    }
}

#[async_trait]
impl CatalogOutbox for PostgresCatalogStore {
    async fn lease(
        &self,
        _worker: &str,
        now_millis: u64,
        lease_millis: u64,
        limit: usize,
    ) -> Result<Vec<OutboxLease>, PortError> {
        let now = datetime_from_millis_port(now_millis)?;
        let until = datetime_from_millis_port(now_millis.saturating_add(lease_millis))?;
        let token = Uuid::now_v7().to_string();
        let rows = sqlx::query(
            "WITH candidates AS ( \
               SELECT event_id FROM catalog_outbox WHERE published_at IS NULL \
                 AND (lease_until IS NULL OR lease_until <= $1) \
               ORDER BY created_at, event_id FOR UPDATE SKIP LOCKED LIMIT $2 \
             ) UPDATE catalog_outbox outbox SET lease_owner = $3, lease_until = $4, \
               attempts = attempts + 1 FROM candidates \
             WHERE outbox.event_id = candidates.event_id \
             RETURNING outbox.event_id, outbox.payload",
        )
        .bind(now)
        .bind(i64::try_from(limit).map_err(|_| PortError::Internal)?)
        .bind(&token)
        .bind(until)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sql)?;
        rows.into_iter()
            .map(|row| {
                Ok(OutboxLease {
                    event_id: row
                        .try_get::<Uuid, _>("event_id")
                        .map_err(|_| PortError::Internal)?
                        .to_string(),
                    payload: row.try_get("payload").map_err(|_| PortError::Internal)?,
                    lease_token: token.clone(),
                })
            })
            .collect()
    }

    async fn mark_published(&self, event_id: &str, lease_token: &str) -> Result<(), PortError> {
        let event_id = Uuid::from_str(event_id).map_err(|_| PortError::Internal)?;
        sqlx::query(
            "UPDATE catalog_outbox SET published_at = clock_timestamp(), lease_owner = NULL, \
             lease_until = NULL WHERE event_id = $1 AND lease_owner = $2",
        )
        .bind(event_id)
        .bind(lease_token)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        Ok(())
    }

    async fn release(&self, event_id: &str, lease_token: &str) -> Result<(), PortError> {
        let event_id = Uuid::from_str(event_id).map_err(|_| PortError::Internal)?;
        sqlx::query(
            "UPDATE catalog_outbox SET lease_owner = NULL, lease_until = NULL \
             WHERE event_id = $1 AND lease_owner = $2 AND published_at IS NULL",
        )
        .bind(event_id)
        .bind(lease_token)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        Ok(())
    }

    async fn delete_published_before(&self, cutoff_millis: u64) -> Result<u64, PortError> {
        let deleted = sqlx::query(
            "DELETE FROM catalog_outbox WHERE published_at IS NOT NULL AND published_at < $1",
        )
        .bind(datetime_from_millis_port(cutoff_millis)?)
        .execute(&self.pool)
        .await
        .map_err(map_sql)?;
        Ok(deleted.rows_affected())
    }
}

fn image_from_row(row: &sqlx::postgres::PgRow) -> Result<LotImage, PortError> {
    let image_id = ImageId::new(
        row.try_get::<String, _>("image_id")
            .map_err(|_| PortError::Internal)?,
    )
    .map_err(|_| PortError::Internal)?;
    let owner = PrincipalId::new(
        row.try_get::<String, _>("owner_id")
            .map_err(|_| PortError::Internal)?,
    )
    .map_err(|_| PortError::Internal)?;
    let expected = ObjectMetadata::new(
        row.try_get::<String, _>("expected_content_type")
            .map_err(|_| PortError::Internal)?,
        u64::try_from(
            row.try_get::<i64, _>("expected_content_length")
                .map_err(|_| PortError::Internal)?,
        )
        .map_err(|_| PortError::Internal)?,
        row.try_get::<String, _>("expected_checksum_sha256")
            .map_err(|_| PortError::Internal)?,
    )
    .map_err(|_| PortError::Internal)?;
    let state_name: String = row.try_get("state").map_err(|_| PortError::Internal)?;
    let state = match state_name.as_str() {
        "PENDING" => ImageState::Pending,
        "INVALID" => ImageState::Invalid,
        "READY" => ImageState::Ready {
            ready_at_millis: datetime_millis(
                row.try_get("ready_at").map_err(|_| PortError::Internal)?,
            )?,
        },
        "ATTACHED" => ImageState::Attached {
            lot_id: LotId::new(
                row.try_get::<String, _>("lot_id")
                    .map_err(|_| PortError::Internal)?,
            )
            .map_err(|_| PortError::Internal)?,
            position: u8::try_from(
                row.try_get::<i16, _>("position")
                    .map_err(|_| PortError::Internal)?,
            )
            .map_err(|_| PortError::Internal)?,
            alt: AltText::new(
                row.try_get::<String, _>("alt_text")
                    .map_err(|_| PortError::Internal)?,
            )
            .map_err(|_| PortError::Internal)?,
        },
        _ => return Err(PortError::Internal),
    };
    let expires_at: DateTime<Utc> = row.try_get("expires_at").map_err(|_| PortError::Internal)?;
    LotImage::restore(
        image_id,
        owner,
        row.try_get::<String, _>("object_key")
            .map_err(|_| PortError::Internal)?,
        expected,
        datetime_millis(expires_at)?,
        state,
    )
    .map_err(|_| PortError::Internal)
}

fn datetime_from_millis_port(value: u64) -> Result<DateTime<Utc>, PortError> {
    let value = i64::try_from(value).map_err(|_| PortError::Internal)?;
    DateTime::from_timestamp_millis(value).ok_or(PortError::Internal)
}

fn datetime_millis(value: DateTime<Utc>) -> Result<u64, PortError> {
    u64::try_from(value.timestamp_millis()).map_err(|_| PortError::Internal)
}

async fn complete_receipt<T: serde::Serialize>(
    transaction: &mut Transaction<'_, Postgres>,
    receipt: &crate::application::ReceiptIdentity,
    result: &T,
) -> Result<(), CatalogCommitError> {
    let payload = serde_json::to_value(result).map_err(|_| internal_commit())?;
    let updated = sqlx::query(
        "UPDATE catalog_command_receipts SET status = 'COMPLETED', result_payload = $3, \
         updated_at = clock_timestamp() WHERE caller_scope = $1 AND idempotency_key = $2 \
         AND status = 'CLAIMED'",
    )
    .bind(&receipt.caller_scope)
    .bind(&receipt.idempotency_key)
    .bind(payload)
    .execute(&mut **transaction)
    .await
    .map_err(map_commit_sql)?;
    if updated.rows_affected() != 1 {
        return Err(CatalogCommitError::Conflict);
    }
    Ok(())
}

async fn insert_listed_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    commit: &ListLotCommit,
) -> Result<(), CatalogCommitError> {
    let (event_id, correlation_id, occurred_at_millis) = event_parts(&commit.event, "LotListed")?;
    let payload = encode(&EventEnvelope {
        event_id: event_id.to_string(),
        event_type: "LotListed".to_owned(),
        schema_version: 1,
        aggregate_version: commit.lot.version(),
        occurred_at_millis,
        correlation_id: correlation_id.to_owned(),
        payload: LotListedV1 {
            auction_id: commit.lot.lot_id().to_string(),
            lot_id: commit.lot.lot_id().to_string(),
            seller_id: commit.lot.seller().to_string(),
            start_price_minor: commit.lot.start_price().minor(),
            currency_code: commit.lot.start_price().currency().as_str().to_owned(),
            bid_step_minor: commit.lot.bid_step().minor(),
            closing_policy: ClosingPolicyV1 {
                scheduled_end_millis: Some(commit.lot.closing_policy().scheduled_end_millis()),
                inactivity_millis: commit.lot.closing_policy().inactivity_millis(),
                hard_deadline_millis: commit.lot.closing_policy().hard_deadline_millis(),
            },
        },
    })
    .map_err(|_| internal_commit())?;
    insert_outbox(
        transaction,
        event_id,
        commit.lot.lot_id().as_str(),
        commit.lot.version(),
        "LotListed",
        occurred_at_millis,
        &payload,
        correlation_id,
    )
    .await
}

async fn insert_updated_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    commit: &UpdateLotCommit,
    version: u64,
    title: &str,
    description: Option<String>,
) -> Result<(), CatalogCommitError> {
    let (event_id, correlation_id, occurred_at_millis) = event_parts(&commit.event, "LotUpdated")?;
    let payload = encode(&EventEnvelope {
        event_id: event_id.to_string(),
        event_type: "LotUpdated".to_owned(),
        schema_version: 1,
        aggregate_version: version,
        occurred_at_millis,
        correlation_id: correlation_id.to_owned(),
        payload: LotUpdatedV1 {
            lot_id: commit.lot_id.to_string(),
            title: title.to_owned(),
            description,
        },
    })
    .map_err(|_| internal_commit())?;
    insert_outbox(
        transaction,
        event_id,
        commit.lot_id.as_str(),
        version,
        "LotUpdated",
        occurred_at_millis,
        &payload,
        correlation_id,
    )
    .await
}

async fn insert_withdrawn_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    commit: &WithdrawLotCommit,
    version: u64,
) -> Result<(), CatalogCommitError> {
    let (event_id, correlation_id, occurred_at_millis) =
        event_parts(&commit.event, "LotWithdrawn")?;
    let payload = encode(&EventEnvelope {
        event_id: event_id.to_string(),
        event_type: "LotWithdrawn".to_owned(),
        schema_version: 1,
        aggregate_version: version,
        occurred_at_millis,
        correlation_id: correlation_id.to_owned(),
        payload: LotWithdrawnV1 {
            lot_id: commit.lot_id.to_string(),
        },
    })
    .map_err(|_| internal_commit())?;
    insert_outbox(
        transaction,
        event_id,
        commit.lot_id.as_str(),
        version,
        "LotWithdrawn",
        occurred_at_millis,
        &payload,
        correlation_id,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
    aggregate_id: &str,
    aggregate_version: u64,
    event_type: &str,
    occurred_at_millis: u64,
    payload: &[u8],
    correlation_id: &str,
) -> Result<(), CatalogCommitError> {
    sqlx::query(
        "INSERT INTO catalog_outbox \
         (event_id, aggregate_id, aggregate_version, event_type, schema_version, occurred_at, \
          payload, correlation_id) VALUES ($1, $2, $3, $4, 1, $5, $6, $7)",
    )
    .bind(event_id)
    .bind(aggregate_id)
    .bind(to_i64(aggregate_version)?)
    .bind(event_type)
    .bind(millis_to_datetime(occurred_at_millis)?)
    .bind(payload)
    .bind(correlation_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_commit_sql)?;
    Ok(())
}

fn event_parts<'a>(
    event: &'a CatalogEvent,
    expected: &str,
) -> Result<(Uuid, &'a str, u64), CatalogCommitError> {
    let (event_id, correlation_id, occurred_at_millis, actual) = match event {
        CatalogEvent::LotListed {
            event_id,
            correlation_id,
            occurred_at_millis,
        } => (event_id, correlation_id, *occurred_at_millis, "LotListed"),
        CatalogEvent::LotUpdated {
            event_id,
            correlation_id,
            occurred_at_millis,
        } => (event_id, correlation_id, *occurred_at_millis, "LotUpdated"),
        CatalogEvent::LotWithdrawn {
            event_id,
            correlation_id,
            occurred_at_millis,
        } => (
            event_id,
            correlation_id,
            *occurred_at_millis,
            "LotWithdrawn",
        ),
    };
    if actual != expected {
        return Err(internal_commit());
    }
    Ok((
        Uuid::from_str(event_id).map_err(|_| internal_commit())?,
        correlation_id,
        occurred_at_millis,
    ))
}

fn millis_to_datetime(value: u64) -> Result<DateTime<Utc>, CatalogCommitError> {
    DateTime::from_timestamp_millis(to_i64(value)?).ok_or_else(internal_commit)
}

fn optional_i64(value: Option<u64>) -> Result<Option<i64>, CatalogCommitError> {
    value.map(to_i64).transpose()
}

fn to_i64(value: u64) -> Result<i64, CatalogCommitError> {
    i64::try_from(value).map_err(|_| internal_commit())
}

fn to_u64(value: i64) -> Result<u64, CatalogCommitError> {
    u64::try_from(value).map_err(|_| internal_commit())
}

fn internal_commit() -> CatalogCommitError {
    CatalogCommitError::Port(PortError::Internal)
}

fn map_sql(error: sqlx::Error) -> PortError {
    match error {
        sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) | sqlx::Error::Tls(_) => {
            PortError::Unavailable
        }
        sqlx::Error::RowNotFound => PortError::NotFound,
        _ => PortError::Internal,
    }
}

fn map_commit_sql(error: sqlx::Error) -> CatalogCommitError {
    CatalogCommitError::Port(map_sql(error))
}
