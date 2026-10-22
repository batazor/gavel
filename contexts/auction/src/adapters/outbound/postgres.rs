use async_trait::async_trait;
use chrono::{DateTime, Utc};
use prost::Message;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, types::Json};
use uuid::Uuid;

use crate::{
    application::{
        AuctionProjection, AuctionProjectionReader, AuctionUnitOfWork, AuctionView,
        CommandIdentity, CommandLookup, CommandResult, CommitError, CommitOutcome, CommitRequest,
        Inbox, InboxError, InboxOutcome, LoadedAuction, MoneyView, PaymentReadiness,
        PaymentReadinessStore, PaymentReadinessUpdate, PaymentReadinessUpdateOutcome, PortError,
        PublicAuctionStatus, ReconciliationOperation, ReconciliationQueue,
    },
    domain::{AuctionEvent, Money, PublicEvent, Timestamp},
    rpc,
    telemetry::{InboxMetricOutcome, record_inbox, record_outbox, record_replay},
};

use super::codec::{PrivateEventCodec, SnapshotCodec, StoredPrivateEvent, StoredSnapshot};

#[derive(Clone)]
pub struct PostgresAuctionStore {
    pool: PgPool,
}

impl PostgresAuctionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl AuctionUnitOfWork for PostgresAuctionStore {
    async fn load(&self, auction_id: &str) -> Result<Option<LoadedAuction>, CommitError> {
        let mut transaction = self.pool.begin().await.map_err(map_sqlx_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_sqlx_error)?;

        let current_version: Option<i64> =
            sqlx::query_scalar("SELECT current_version FROM auction_streams WHERE auction_id = $1")
                .bind(auction_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(map_sqlx_error)?;
        let Some(current_version) = current_version else {
            transaction.commit().await.map_err(map_sqlx_error)?;
            return Ok(None);
        };
        let current_version = to_u64(current_version)?;

        let snapshot_row = sqlx::query(
            "SELECT version, snapshot_schema_version, payload \
             FROM auction_snapshots WHERE auction_id = $1 AND version <= $2",
        )
        .bind(auction_id)
        .bind(to_i64(current_version)?)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;

        let mut snapshot = None;
        let mut replay_after = 0_u64;
        if let Some(row) = snapshot_row {
            let snapshot_version =
                to_u64(row.try_get::<i64, _>("version").map_err(map_sqlx_error)?)?;
            let stored = StoredSnapshot {
                schema_version: row
                    .try_get::<i32, _>("snapshot_schema_version")
                    .map_err(map_sqlx_error)?
                    .try_into()
                    .map_err(|_| CommitError::Internal)?,
                payload: row
                    .try_get::<Json<Value>, _>("payload")
                    .map_err(map_sqlx_error)?
                    .0,
            };
            if let Some(decoded) =
                SnapshotCodec::decode(stored).map_err(|_| CommitError::Internal)?
            {
                snapshot = Some(decoded);
                replay_after = snapshot_version;
            }
        }

        let rows = sqlx::query(
            "SELECT event_type, event_schema_version, payload \
             FROM auction_events \
             WHERE auction_id = $1 AND version > $2 AND version <= $3 \
             ORDER BY version ASC",
        )
        .bind(auction_id)
        .bind(to_i64(replay_after)?)
        .bind(to_i64(current_version)?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;

        let events = rows
            .into_iter()
            .map(|row| {
                let schema_version = row
                    .try_get::<i32, _>("event_schema_version")
                    .map_err(map_sqlx_error)?
                    .try_into()
                    .map_err(|_| CommitError::Internal)?;
                PrivateEventCodec::decode(StoredPrivateEvent {
                    event_type: row.try_get("event_type").map_err(map_sqlx_error)?,
                    schema_version,
                    payload: row
                        .try_get::<Json<Value>, _>("payload")
                        .map_err(map_sqlx_error)?
                        .0,
                })
                .map_err(|_| CommitError::Internal)
            })
            .collect::<Result<Vec<_>, _>>()?;
        record_replay(events.len());

        transaction.commit().await.map_err(map_sqlx_error)?;
        Ok(Some(LoadedAuction {
            snapshot,
            events,
            version: current_version,
        }))
    }

    async fn lookup_command(
        &self,
        identity: &CommandIdentity,
    ) -> Result<CommandLookup, CommitError> {
        lookup_command(&self.pool, identity).await
    }

    async fn commit(&self, request: CommitRequest) -> Result<CommitOutcome, CommitError> {
        validate_request(&request)?;
        let mut transaction = self.pool.begin().await.map_err(map_sqlx_error)?;

        if let Some((identity, _)) = &request.command {
            let lock_key = format!(
                "{}:{}{}",
                identity.principal_scope.len(),
                identity.principal_scope,
                identity.idempotency_key
            );
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(lock_key)
                .execute(&mut *transaction)
                .await
                .map_err(map_sqlx_error)?;
            match lookup_command(&mut *transaction, identity).await? {
                CommandLookup::Replay(result) => {
                    transaction.commit().await.map_err(map_sqlx_error)?;
                    return Ok(CommitOutcome::Replayed { result });
                }
                CommandLookup::Conflict => return Err(CommitError::IdempotencyConflict),
                CommandLookup::Missing => {}
            }
        }

        sqlx::query(
            "INSERT INTO auction_streams (auction_id, current_version) VALUES ($1, 0) \
             ON CONFLICT (auction_id) DO NOTHING",
        )
        .bind(&request.auction_id)
        .execute(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;

        let current_version: i64 = sqlx::query_scalar(
            "SELECT current_version FROM auction_streams WHERE auction_id = $1 FOR UPDATE",
        )
        .bind(&request.auction_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;
        if to_u64(current_version)? != request.expected_version {
            return Err(CommitError::VersionConflict);
        }

        let fallback_time = request
            .projection
            .as_ref()
            .map_or_else(Utc::now, |projection| {
                timestamp_to_chrono(projection.updated_at).unwrap_or_else(|_| Utc::now())
            });
        for (offset, event) in request.private_events.iter().enumerate() {
            let version = request
                .expected_version
                .checked_add(offset as u64 + 1)
                .ok_or(CommitError::InvalidRequest)?;
            let stored = PrivateEventCodec::encode(event).map_err(|_| CommitError::Internal)?;
            sqlx::query(
                "INSERT INTO auction_events \
                 (auction_id, version, event_id, event_type, event_schema_version, payload, occurred_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(&request.auction_id)
            .bind(to_i64(version)?)
            .bind(Uuid::now_v7())
            .bind(stored.event_type)
            .bind(i32::try_from(stored.schema_version).map_err(|_| CommitError::InvalidRequest)?)
            .bind(Json(stored.payload))
            .bind(private_event_time(event).unwrap_or(fallback_time))
            .execute(&mut *transaction)
            .await
            .map_err(map_sqlx_error)?;
        }

        let resulting_version = request
            .expected_version
            .checked_add(request.private_events.len() as u64)
            .ok_or(CommitError::InvalidRequest)?;
        sqlx::query(
            "UPDATE auction_streams SET current_version = $2, updated_at = clock_timestamp() \
             WHERE auction_id = $1",
        )
        .bind(&request.auction_id)
        .bind(to_i64(resulting_version)?)
        .execute(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;

        if let Some(projection) = &request.projection {
            upsert_projection(&mut transaction, projection).await?;
        }

        let (correlation_id, causation_id) = request
            .command
            .as_ref()
            .map(|(identity, _)| {
                (
                    identity.correlation_id.as_str(),
                    identity.causation_id.as_str(),
                )
            })
            .unwrap_or(("system", "system"));
        for event in &request.public_events {
            insert_public_event(
                &mut transaction,
                event,
                fallback_time,
                correlation_id,
                causation_id,
            )
            .await?;
        }

        if let Some((identity, result)) = &request.command {
            let fingerprint = decode_fingerprint(&identity.request_fingerprint)?;
            let result_payload = serde_json::to_value(result).map_err(|_| CommitError::Internal)?;
            sqlx::query(
                "INSERT INTO auction_command_results \
                 (principal_scope, idempotency_key, request_fingerprint, operation, result_type, result_payload, stream_version) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(&identity.principal_scope)
            .bind(&identity.idempotency_key)
            .bind(fingerprint)
            .bind(&identity.operation)
            .bind(command_result_type(result))
            .bind(Json(result_payload))
            .bind(to_i64(resulting_version)?)
            .execute(&mut *transaction)
            .await
            .map_err(map_sqlx_error)?;
        }

        for operation in &request.reconciliation {
            sqlx::query(
                "INSERT INTO auction_reconciliation_queue \
                 (operation_id, resource_type, resource_id, relation, subject_type, subject_id) \
                 VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (operation_id) DO NOTHING",
            )
            .bind(&operation.operation_id)
            .bind(&operation.resource_type)
            .bind(&operation.resource_id)
            .bind(&operation.relation)
            .bind(&operation.subject_type)
            .bind(&operation.subject_id)
            .execute(&mut *transaction)
            .await
            .map_err(map_sqlx_error)?;
        }

        transaction.commit().await.map_err(map_sqlx_error)?;
        record_outbox(request.public_events.len());
        Ok(CommitOutcome::Committed {
            version: resulting_version,
        })
    }
}

#[async_trait]
impl Inbox for PostgresAuctionStore {
    async fn accept(
        &self,
        consumer: &str,
        event_id: &str,
        payload_fingerprint: &str,
    ) -> Result<InboxOutcome, InboxError> {
        if consumer.trim().is_empty() || event_id.trim().is_empty() {
            return Err(InboxError::Internal);
        }
        let fingerprint =
            decode_fingerprint(payload_fingerprint).map_err(|_| InboxError::Internal)?;
        let mut transaction = self.pool.begin().await.map_err(map_inbox_sqlx_error)?;
        let lock_key = format!("{}:{}{}", consumer.len(), consumer, event_id);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await
            .map_err(map_inbox_sqlx_error)?;
        let existing: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT payload_fingerprint FROM auction_inbox \
             WHERE consumer = $1 AND event_id = $2 FOR UPDATE",
        )
        .bind(consumer)
        .bind(event_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_inbox_sqlx_error)?;
        let outcome = match existing {
            Some(stored) if stored == fingerprint => InboxOutcome::Duplicate,
            Some(_) => {
                sqlx::query(
                    "UPDATE auction_inbox SET status = 'QUARANTINED', processed_at = clock_timestamp() \
                     WHERE consumer = $1 AND event_id = $2",
                )
                .bind(consumer)
                .bind(event_id)
                .execute(&mut *transaction)
                .await
                .map_err(map_inbox_sqlx_error)?;
                InboxOutcome::PayloadConflict
            }
            None => {
                sqlx::query(
                    "INSERT INTO auction_inbox (consumer, event_id, payload_fingerprint) \
                     VALUES ($1, $2, $3)",
                )
                .bind(consumer)
                .bind(event_id)
                .bind(fingerprint)
                .execute(&mut *transaction)
                .await
                .map_err(map_inbox_sqlx_error)?;
                InboxOutcome::Accepted
            }
        };
        transaction.commit().await.map_err(map_inbox_sqlx_error)?;
        record_inbox(match outcome {
            InboxOutcome::Accepted => InboxMetricOutcome::Accepted,
            InboxOutcome::Duplicate => InboxMetricOutcome::Duplicate,
            InboxOutcome::PayloadConflict => InboxMetricOutcome::Conflict,
        });
        Ok(outcome)
    }
}

#[async_trait]
impl PaymentReadiness for PostgresAuctionStore {
    async fn is_ready(&self, user_id: &str) -> Result<bool, PortError> {
        sqlx::query_scalar("SELECT ready FROM payment_readiness WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map(|value| value.unwrap_or(false))
            .map_err(map_port_sqlx_error)
    }
}

#[async_trait]
impl PaymentReadinessStore for PostgresAuctionStore {
    async fn apply_update(
        &self,
        update: PaymentReadinessUpdate,
    ) -> Result<PaymentReadinessUpdateOutcome, PortError> {
        if update.user_id.trim().is_empty() || update.source_event_id.trim().is_empty() {
            return Err(PortError::InvalidInput);
        }
        let source_version = to_i64(update.source_version).map_err(|_| PortError::InvalidInput)?;
        let updated_at =
            timestamp_to_chrono(update.occurred_at).map_err(|_| PortError::InvalidInput)?;
        let mut transaction = self.pool.begin().await.map_err(map_port_sqlx_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("payment-readiness:{}", update.user_id))
            .execute(&mut *transaction)
            .await
            .map_err(map_port_sqlx_error)?;
        let existing = sqlx::query(
            "SELECT source_version, source_event_id FROM payment_readiness \
             WHERE user_id = $1 FOR UPDATE",
        )
        .bind(&update.user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_port_sqlx_error)?;
        let outcome = if let Some(row) = existing {
            let stored_version: i64 = row.try_get("source_version").map_err(map_port_sqlx_error)?;
            let stored_event_id: String = row
                .try_get("source_event_id")
                .map_err(map_port_sqlx_error)?;
            if stored_version == source_version && stored_event_id == update.source_event_id {
                PaymentReadinessUpdateOutcome::Duplicate
            } else if stored_version >= source_version {
                PaymentReadinessUpdateOutcome::Stale
            } else {
                update_payment_readiness(&mut transaction, &update, source_version, updated_at)
                    .await?;
                PaymentReadinessUpdateOutcome::Applied
            }
        } else {
            update_payment_readiness(&mut transaction, &update, source_version, updated_at).await?;
            PaymentReadinessUpdateOutcome::Applied
        };
        transaction.commit().await.map_err(map_port_sqlx_error)?;
        Ok(outcome)
    }
}

#[async_trait]
impl ReconciliationQueue for PostgresAuctionStore {
    async fn pending(&self, limit: usize) -> Result<Vec<ReconciliationOperation>, PortError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).map_err(|_| PortError::InvalidInput)?;
        let rows = sqlx::query(
            "SELECT operation_id, resource_type, resource_id, relation, subject_type, subject_id \
             FROM auction_reconciliation_queue \
             WHERE completed_at IS NULL AND available_at <= clock_timestamp() \
             ORDER BY available_at, created_at LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_port_sqlx_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(ReconciliationOperation {
                    operation_id: row.try_get("operation_id").map_err(map_port_sqlx_error)?,
                    resource_type: row.try_get("resource_type").map_err(map_port_sqlx_error)?,
                    resource_id: row.try_get("resource_id").map_err(map_port_sqlx_error)?,
                    relation: row.try_get("relation").map_err(map_port_sqlx_error)?,
                    subject_type: row.try_get("subject_type").map_err(map_port_sqlx_error)?,
                    subject_id: row.try_get("subject_id").map_err(map_port_sqlx_error)?,
                })
            })
            .collect()
    }

    async fn complete(&self, operation_id: &str) -> Result<(), PortError> {
        sqlx::query(
            "UPDATE auction_reconciliation_queue SET completed_at = COALESCE(completed_at, clock_timestamp()) \
             WHERE operation_id = $1",
        )
        .bind(operation_id)
        .execute(&self.pool)
        .await
        .map_err(map_port_sqlx_error)?;
        Ok(())
    }
}

#[async_trait]
impl AuctionProjectionReader for PostgresAuctionStore {
    async fn auction(&self, auction_id: &str) -> Result<Option<AuctionProjection>, PortError> {
        let row = sqlx::query(
            "SELECT auction_id, lot_id, status, amount_minor::text AS amount_minor, \
                    currency_code, ends_at, stream_version, updated_at \
             FROM auction_public_projection WHERE auction_id = $1",
        )
        .bind(auction_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_port_sqlx_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let amount = row
            .try_get::<String, _>("amount_minor")
            .map_err(map_port_sqlx_error)?
            .parse::<u64>()
            .map_err(|_| PortError::Internal)?;
        let status = parse_public_status(
            &row.try_get::<String, _>("status")
                .map_err(map_port_sqlx_error)?,
        )?;
        let ends_at: DateTime<Utc> = row.try_get("ends_at").map_err(map_port_sqlx_error)?;
        let updated_at: DateTime<Utc> = row.try_get("updated_at").map_err(map_port_sqlx_error)?;
        Ok(Some(AuctionProjection {
            view: AuctionView {
                auction_id: row.try_get("auction_id").map_err(map_port_sqlx_error)?,
                lot_id: row.try_get("lot_id").map_err(map_port_sqlx_error)?,
                status,
                current_price: MoneyView {
                    minor_units: amount,
                    currency_code: row.try_get("currency_code").map_err(map_port_sqlx_error)?,
                },
                ends_at_millis: chrono_to_timestamp(ends_at)?,
                version: to_u64(
                    row.try_get::<i64, _>("stream_version")
                        .map_err(map_port_sqlx_error)?,
                )
                .map_err(|_| PortError::Internal)?,
            },
            updated_at: Timestamp::from_millis(chrono_to_timestamp(updated_at)?),
        }))
    }
}

async fn update_payment_readiness(
    transaction: &mut Transaction<'_, Postgres>,
    update: &PaymentReadinessUpdate,
    source_version: i64,
    updated_at: DateTime<Utc>,
) -> Result<(), PortError> {
    sqlx::query(
        "INSERT INTO payment_readiness \
         (user_id, ready, source_version, source_event_id, updated_at) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (user_id) DO UPDATE SET ready = EXCLUDED.ready, \
           source_version = EXCLUDED.source_version, source_event_id = EXCLUDED.source_event_id, \
           updated_at = EXCLUDED.updated_at \
         WHERE payment_readiness.source_version < EXCLUDED.source_version",
    )
    .bind(&update.user_id)
    .bind(update.ready)
    .bind(source_version)
    .bind(&update.source_event_id)
    .bind(updated_at)
    .execute(&mut **transaction)
    .await
    .map_err(map_port_sqlx_error)?;
    Ok(())
}

async fn lookup_command<'e, E>(
    executor: E,
    identity: &CommandIdentity,
) -> Result<CommandLookup, CommitError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let row = sqlx::query(
        "SELECT request_fingerprint, result_payload FROM auction_command_results \
         WHERE principal_scope = $1 AND idempotency_key = $2",
    )
    .bind(&identity.principal_scope)
    .bind(&identity.idempotency_key)
    .fetch_optional(executor)
    .await
    .map_err(map_sqlx_error)?;
    let Some(row) = row else {
        return Ok(CommandLookup::Missing);
    };
    let stored: Vec<u8> = row.try_get("request_fingerprint").map_err(map_sqlx_error)?;
    if stored != decode_fingerprint(&identity.request_fingerprint)? {
        return Ok(CommandLookup::Conflict);
    }
    let payload: Json<Value> = row.try_get("result_payload").map_err(map_sqlx_error)?;
    let result = serde_json::from_value(payload.0).map_err(|_| CommitError::Internal)?;
    Ok(CommandLookup::Replay(result))
}

async fn upsert_projection(
    transaction: &mut Transaction<'_, Postgres>,
    projection: &AuctionProjection,
) -> Result<(), CommitError> {
    let view = &projection.view;
    sqlx::query(
        "INSERT INTO auction_public_projection \
         (auction_id, lot_id, status, amount_minor, currency_code, ends_at, stream_version, updated_at) \
         VALUES ($1, $2, $3, $4::numeric, $5, $6, $7, $8) \
         ON CONFLICT (auction_id) DO UPDATE SET \
           lot_id = EXCLUDED.lot_id, status = EXCLUDED.status, amount_minor = EXCLUDED.amount_minor, \
           currency_code = EXCLUDED.currency_code, ends_at = EXCLUDED.ends_at, \
           stream_version = EXCLUDED.stream_version, updated_at = EXCLUDED.updated_at \
         WHERE auction_public_projection.stream_version < EXCLUDED.stream_version",
    )
    .bind(&view.auction_id)
    .bind(&view.lot_id)
    .bind(public_status(view.status))
    .bind(view.current_price.minor_units.to_string())
    .bind(&view.current_price.currency_code)
    .bind(timestamp_to_chrono(Timestamp::from_millis(view.ends_at_millis))?)
    .bind(to_i64(view.version)?)
    .bind(timestamp_to_chrono(projection.updated_at)?)
    .execute(&mut **transaction)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

async fn insert_public_event(
    transaction: &mut Transaction<'_, Postgres>,
    event: &PublicEvent,
    fallback_time: DateTime<Utc>,
    correlation_id: &str,
    causation_id: &str,
) -> Result<(), CommitError> {
    let event_id = Uuid::now_v7();
    let (aggregate_id, aggregate_version, event_type, payload, occurred_at) =
        public_event_parts(event, fallback_time)?;
    let envelope = rpc::AuctionPublicEvent {
        event_id: event_id.to_string(),
        aggregate_id: aggregate_id.clone(),
        aggregate_version,
        schema_version: 1,
        occurred_at: Some(chrono_to_prost(occurred_at)),
        correlation_id: correlation_id.to_owned(),
        causation_id: causation_id.to_owned(),
        payload: Some(payload),
    };
    sqlx::query(
        "INSERT INTO auction_outbox \
         (event_id, aggregate_id, aggregate_version, event_type, schema_version, occurred_at, payload, correlation_id, causation_id) \
         VALUES ($1, $2, $3, $4, 1, $5, $6, $7, $8)",
    )
    .bind(event_id)
    .bind(aggregate_id)
    .bind(to_i64(aggregate_version)?)
    .bind(event_type)
    .bind(occurred_at)
    .bind(envelope.encode_to_vec())
    .bind(correlation_id)
    .bind(causation_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

type PublicPayload = rpc::auction_public_event::Payload;

fn public_event_parts(
    event: &PublicEvent,
    fallback_time: DateTime<Utc>,
) -> Result<(String, u64, &'static str, PublicPayload, DateTime<Utc>), CommitError> {
    let parts = match event {
        PublicEvent::AuctionOpened {
            auction_id,
            ends_at,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "AuctionOpened",
            PublicPayload::AuctionOpened(rpc::AuctionOpenedEvent {
                ends_at: Some(timestamp_to_prost(*ends_at)?),
            }),
            fallback_time,
        ),
        PublicEvent::BidRecorded {
            bid_id,
            auction_id,
            bidder_alias,
            resulting_price,
            occurred_at,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "BidRecorded",
            PublicPayload::BidRecorded(rpc::BidRecordedEvent {
                bid_id: bid_id.to_string(),
                bidder_alias: bidder_alias.to_string(),
                resulting_price: Some(money_to_proto(resulting_price)),
                recorded_at: Some(timestamp_to_prost(*occurred_at)?),
            }),
            timestamp_to_chrono(*occurred_at)?,
        ),
        PublicEvent::PriceChanged {
            auction_id,
            price,
            ends_at,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "PriceChanged",
            PublicPayload::PriceChanged(rpc::PriceChangedEvent {
                price: Some(money_to_proto(price)),
                ends_at: Some(timestamp_to_prost(*ends_at)?),
            }),
            fallback_time,
        ),
        PublicEvent::DeadlineExtended {
            auction_id,
            ends_at,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "DeadlineExtended",
            PublicPayload::DeadlineExtended(rpc::DeadlineExtendedEvent {
                ends_at: Some(timestamp_to_prost(*ends_at)?),
            }),
            fallback_time,
        ),
        PublicEvent::AuctionRescheduled {
            auction_id,
            ends_at,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "AuctionRescheduled",
            PublicPayload::AuctionRescheduled(rpc::AuctionRescheduledEvent {
                ends_at: Some(timestamp_to_prost(*ends_at)?),
            }),
            fallback_time,
        ),
        PublicEvent::AuctionClosed {
            auction_id,
            closing_price,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "AuctionClosed",
            PublicPayload::AuctionClosed(rpc::AuctionClosedEvent {
                closing_price: Some(money_to_proto(closing_price)),
            }),
            fallback_time,
        ),
        PublicEvent::AuctionCancelled {
            auction_id,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "AuctionCancelled",
            PublicPayload::AuctionCancelled(rpc::AuctionCancelledEvent {}),
            fallback_time,
        ),
        PublicEvent::AuctionUnsold {
            auction_id,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "AuctionUnsold",
            PublicPayload::AuctionUnsold(rpc::AuctionUnsoldEvent {}),
            fallback_time,
        ),
        PublicEvent::WinnerConfirmed {
            auction_id,
            version,
        } => (
            auction_id.to_string(),
            *version,
            "WinnerConfirmed",
            PublicPayload::WinnerConfirmed(rpc::WinnerConfirmedEvent {}),
            fallback_time,
        ),
    };
    Ok(parts)
}

fn validate_request(request: &CommitRequest) -> Result<(), CommitError> {
    if request.auction_id.trim().is_empty() {
        return Err(CommitError::InvalidRequest);
    }
    request
        .expected_version
        .checked_add(request.private_events.len() as u64)
        .ok_or(CommitError::InvalidRequest)?;
    if request
        .public_events
        .iter()
        .any(|event| public_version(event) == 0)
    {
        return Err(CommitError::InvalidRequest);
    }
    if let Some((identity, _)) = &request.command
        && (identity.principal_scope.trim().is_empty()
            || identity.idempotency_key.trim().is_empty()
            || identity.operation.trim().is_empty()
            || identity.correlation_id.trim().is_empty()
            || identity.causation_id.trim().is_empty())
    {
        return Err(CommitError::InvalidRequest);
    }
    Ok(())
}

fn public_version(event: &PublicEvent) -> u64 {
    match event {
        PublicEvent::AuctionOpened { version, .. }
        | PublicEvent::BidRecorded { version, .. }
        | PublicEvent::PriceChanged { version, .. }
        | PublicEvent::DeadlineExtended { version, .. }
        | PublicEvent::AuctionRescheduled { version, .. }
        | PublicEvent::AuctionClosed { version, .. }
        | PublicEvent::AuctionCancelled { version, .. }
        | PublicEvent::AuctionUnsold { version, .. }
        | PublicEvent::WinnerConfirmed { version, .. } => *version,
    }
}

fn private_event_time(event: &AuctionEvent) -> Option<DateTime<Utc>> {
    match event {
        AuctionEvent::Opened { opened_at, .. } => timestamp_to_chrono(*opened_at).ok(),
        AuctionEvent::MaximumBidPlaced { placed_at, .. } => timestamp_to_chrono(*placed_at).ok(),
        _ => None,
    }
}

fn money_to_proto(money: &Money) -> rpc::Money {
    rpc::Money {
        minor_units: money.amount_minor(),
        currency_code: money.currency().to_owned(),
    }
}

fn public_status(status: PublicAuctionStatus) -> &'static str {
    match status {
        PublicAuctionStatus::Scheduled => "SCHEDULED",
        PublicAuctionStatus::Open => "OPEN",
        PublicAuctionStatus::Settling => "SETTLING",
        PublicAuctionStatus::Sold => "SOLD",
        PublicAuctionStatus::Unsold => "UNSOLD",
        PublicAuctionStatus::Cancelled => "CANCELLED",
    }
}

fn parse_public_status(status: &str) -> Result<PublicAuctionStatus, PortError> {
    match status {
        "SCHEDULED" => Ok(PublicAuctionStatus::Scheduled),
        "OPEN" => Ok(PublicAuctionStatus::Open),
        "SETTLING" => Ok(PublicAuctionStatus::Settling),
        "SOLD" => Ok(PublicAuctionStatus::Sold),
        "UNSOLD" => Ok(PublicAuctionStatus::Unsold),
        "CANCELLED" => Ok(PublicAuctionStatus::Cancelled),
        _ => Err(PortError::Internal),
    }
}

fn command_result_type(result: &CommandResult) -> &'static str {
    match result {
        CommandResult::Opened { .. } => "opened",
        CommandResult::PlaceBid(_) => "place_bid",
        CommandResult::Auction(_) => "auction",
        CommandResult::Lifecycle(_) => "lifecycle",
    }
}

fn decode_fingerprint(value: &str) -> Result<Vec<u8>, CommitError> {
    if value.len() != 64 {
        return Err(CommitError::InvalidRequest);
    }
    (0..64)
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| CommitError::InvalidRequest)
        })
        .collect()
}

fn timestamp_to_chrono(value: Timestamp) -> Result<DateTime<Utc>, CommitError> {
    let millis = i64::try_from(value.as_millis()).map_err(|_| CommitError::InvalidRequest)?;
    DateTime::from_timestamp_millis(millis).ok_or(CommitError::InvalidRequest)
}

fn timestamp_to_prost(value: Timestamp) -> Result<prost_types::Timestamp, CommitError> {
    Ok(chrono_to_prost(timestamp_to_chrono(value)?))
}

fn chrono_to_prost(value: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: value.timestamp(),
        nanos: value.timestamp_subsec_nanos() as i32,
    }
}

fn chrono_to_timestamp(value: DateTime<Utc>) -> Result<u64, PortError> {
    value
        .timestamp_millis()
        .try_into()
        .map_err(|_| PortError::Internal)
}

fn to_i64(value: u64) -> Result<i64, CommitError> {
    value.try_into().map_err(|_| CommitError::InvalidRequest)
}

fn to_u64(value: i64) -> Result<u64, CommitError> {
    value.try_into().map_err(|_| CommitError::Internal)
}

fn map_sqlx_error(error: sqlx::Error) -> CommitError {
    match error {
        sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_) => CommitError::Unavailable,
        _ => CommitError::Internal,
    }
}

fn map_inbox_sqlx_error(error: sqlx::Error) -> InboxError {
    match error {
        sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_) => InboxError::Unavailable,
        _ => InboxError::Internal,
    }
}

fn map_port_sqlx_error(error: sqlx::Error) -> PortError {
    match error {
        sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_) => PortError::Unavailable,
        _ => PortError::Internal,
    }
}
