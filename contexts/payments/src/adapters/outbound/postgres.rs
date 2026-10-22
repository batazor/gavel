use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction, types::Json};
use uuid::Uuid;

use crate::{
    application::{
        CallbackClaim, CallbackInbox, Commit, CommitConflict, PaymentsUnitOfWork, PortError,
        ReceiptClaim, ReceiptClaimResult, ReceiptStore,
    },
    domain::{
        DomainEvent, PaymentAttempt, PaymentAttemptId, PaymentAttemptState, PaymentInstrument,
        PaymentInstrumentId, PaymentInstrumentState, PrincipalId,
    },
};

#[derive(Clone)]
pub struct PostgresPayments {
    pool: PgPool,
}

impl PostgresPayments {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
    pub async fn outbox_backlog_age_seconds(&self) -> Result<f64, PortError> {
        sqlx::query_scalar::<_, f64>("SELECT COALESCE(EXTRACT(EPOCH FROM now() - MIN(occurred_at)), 0)::float8 FROM payment_outbox WHERE published_at IS NULL")
            .fetch_one(&self.pool).await.map_err(map_sql)
    }
}

#[async_trait]
impl PaymentsUnitOfWork for PostgresPayments {
    async fn instrument_for_owner(
        &self,
        owner: &PrincipalId,
    ) -> Result<Option<PaymentInstrument>, PortError> {
        sqlx::query_scalar::<_, Json<PaymentInstrument>>(
            "SELECT aggregate FROM payment_instruments WHERE owner_id = $1",
        )
        .bind(owner.as_str())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|Json(value)| value))
        .map_err(map_sql)
    }
    async fn instrument(
        &self,
        id: &PaymentInstrumentId,
    ) -> Result<Option<PaymentInstrument>, PortError> {
        sqlx::query_scalar::<_, Json<PaymentInstrument>>(
            "SELECT aggregate FROM payment_instruments WHERE instrument_id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|Json(value)| value))
        .map_err(map_sql)
    }
    async fn instrument_by_session_ref(
        &self,
        session_ref: &crate::domain::ProviderReference,
    ) -> Result<Option<PaymentInstrument>, PortError> {
        sqlx::query_scalar::<_, Json<PaymentInstrument>>(
            "SELECT aggregate FROM payment_instruments WHERE provider_session_ref=$1",
        )
        .bind(session_ref.expose())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|Json(value)| value))
        .map_err(map_sql)
    }
    async fn save_instrument(
        &self,
        commit: Commit<PaymentInstrument>,
        expected_version: Option<u64>,
    ) -> Result<(), CommitConflict> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| CommitConflict::Version)?;
        let affected = if let Some(expected) = expected_version {
            sqlx::query("UPDATE payment_instruments SET state=$2, version=$3, aggregate=$4, updated_at=now() WHERE instrument_id=$1 AND version=$5")
                .bind(commit.value.id().as_str()).bind(instrument_state(commit.value.state())).bind(to_i64(commit.value.version())?).bind(Json(&commit.value)).bind(to_i64(expected)?).execute(&mut *tx).await.map_err(|_| CommitConflict::Version)?.rows_affected()
        } else {
            sqlx::query("INSERT INTO payment_instruments(instrument_id,owner_id,provider_session_ref,state,version,aggregate) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING")
                .bind(commit.value.id().as_str()).bind(commit.value.owner_id().as_str()).bind(commit.value.provider_session_ref().expose()).bind(instrument_state(commit.value.state())).bind(to_i64(commit.value.version())?).bind(Json(&commit.value)).execute(&mut *tx).await.map_err(|_| CommitConflict::Version)?.rows_affected()
        };
        if affected != 1 {
            return Err(CommitConflict::Version);
        }
        append_events(&mut tx, &commit.events).await?;
        tx.commit().await.map_err(|_| CommitConflict::Version)
    }
    async fn attempt(&self, id: &PaymentAttemptId) -> Result<Option<PaymentAttempt>, PortError> {
        sqlx::query_scalar::<_, Json<PaymentAttempt>>(
            "SELECT aggregate FROM payment_attempts WHERE attempt_id=$1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|Json(value)| value))
        .map_err(map_sql)
    }
    async fn attempt_for_round(
        &self,
        auction_id: &str,
        round: u32,
    ) -> Result<Option<PaymentAttempt>, PortError> {
        sqlx::query_scalar::<_, Json<PaymentAttempt>>(
            "SELECT aggregate FROM payment_attempts WHERE auction_id=$1 AND settlement_round=$2",
        )
        .bind(auction_id)
        .bind(i32::try_from(round).map_err(|_| PortError::Internal)?)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|Json(value)| value))
        .map_err(map_sql)
    }
    async fn attempt_by_operation_ref(
        &self,
        operation_ref: &crate::domain::ProviderReference,
    ) -> Result<Option<PaymentAttempt>, PortError> {
        sqlx::query_scalar::<_, Json<PaymentAttempt>>(
            "SELECT aggregate FROM payment_attempts WHERE provider_operation_ref=$1",
        )
        .bind(operation_ref.expose())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|Json(value)| value))
        .map_err(map_sql)
    }
    async fn save_attempt(
        &self,
        commit: Commit<PaymentAttempt>,
        expected_version: Option<u64>,
    ) -> Result<(), CommitConflict> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| CommitConflict::Version)?;
        let result = if let Some(expected) = expected_version {
            sqlx::query("UPDATE payment_attempts SET state=$2,version=$3,aggregate=$4,provider_operation_ref=$5,updated_at=now() WHERE attempt_id=$1 AND version=$6")
                .bind(commit.value.id().as_str()).bind(attempt_state(commit.value.state())).bind(to_i64(commit.value.version())?).bind(Json(&commit.value)).bind(commit.value.provider_operation_ref().map(|value| value.expose())).bind(to_i64(expected)?).execute(&mut *tx).await
        } else {
            sqlx::query("INSERT INTO payment_attempts(attempt_id,auction_id,settlement_round,candidate_id,state,version,aggregate,provider_operation_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
                .bind(commit.value.id().as_str()).bind(commit.value.auction_id().as_str()).bind(i32::try_from(commit.value.settlement_round().get()).map_err(|_| CommitConflict::Version)?).bind(commit.value.candidate_id().as_str()).bind(attempt_state(commit.value.state())).bind(to_i64(commit.value.version())?).bind(Json(&commit.value)).bind(commit.value.provider_operation_ref().map(|value| value.expose())).execute(&mut *tx).await
        };
        match result {
            Ok(done) if done.rows_affected() == 1 => {}
            Ok(_) => return Err(CommitConflict::Version),
            Err(error)
                if constraint(&error, "payment_attempts_auction_id_settlement_round_key") =>
            {
                return Err(CommitConflict::AttemptForRound);
            }
            Err(error) if constraint(&error, "payment_attempts_one_success_per_auction_idx") => {
                return Err(CommitConflict::SuccessForAuction);
            }
            Err(_) => return Err(CommitConflict::Version),
        }
        append_events(&mut tx, &commit.events).await?;
        tx.commit().await.map_err(|_| CommitConflict::Version)
    }
    async fn stale_attempts(
        &self,
        before_millis: u64,
        limit: usize,
    ) -> Result<Vec<PaymentAttempt>, PortError> {
        let before = DateTime::<Utc>::from_timestamp_millis(
            i64::try_from(before_millis).map_err(|_| PortError::Internal)?,
        )
        .ok_or(PortError::Internal)?;
        let rows = sqlx::query_scalar::<_, Json<PaymentAttempt>>("SELECT aggregate FROM payment_attempts WHERE state IN ('processing','recovery_required') AND updated_at < $1 ORDER BY updated_at,attempt_id LIMIT $2")
            .bind(before).bind(i64::try_from(limit.min(500)).map_err(|_| PortError::Internal)?).fetch_all(&self.pool).await.map_err(map_sql)?;
        Ok(rows.into_iter().map(|Json(value)| value).collect())
    }
    async fn revoked_instruments(&self, limit: usize) -> Result<Vec<PaymentInstrument>, PortError> {
        let rows = sqlx::query_scalar::<_, Json<PaymentInstrument>>("SELECT aggregate FROM payment_instruments WHERE state='revoked' ORDER BY updated_at,instrument_id LIMIT $1")
            .bind(i64::try_from(limit.min(500)).map_err(|_| PortError::Internal)?).fetch_all(&self.pool).await.map_err(map_sql)?;
        Ok(rows.into_iter().map(|Json(value)| value).collect())
    }
}

#[async_trait]
impl ReceiptStore for PostgresPayments {
    async fn claim(&self, claim: ReceiptClaim) -> Result<ReceiptClaimResult, PortError> {
        let token = Uuid::now_v7();
        let lease = DateTime::<Utc>::from_timestamp_millis(
            i64::try_from(claim.lease_until_millis).map_err(|_| PortError::Internal)?,
        )
        .ok_or(PortError::Internal)?;
        let mut tx = self.pool.begin().await.map_err(map_sql)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "{}:{}:{}",
                claim.scope, claim.command, claim.key_digest
            ))
            .execute(&mut *tx)
            .await
            .map_err(map_sql)?;
        let row = sqlx::query("SELECT fingerprint,claim_token,lease_until,result_payload FROM payment_command_receipts WHERE scope=$1 AND command=$2 AND key_digest=$3 FOR UPDATE")
            .bind(&claim.scope).bind(claim.command).bind(&claim.key_digest).fetch_optional(&mut *tx).await.map_err(map_sql)?;
        let outcome = if let Some(row) = row {
            let fingerprint: String = row
                .try_get("fingerprint")
                .map_err(|_| PortError::Internal)?;
            if fingerprint != claim.fingerprint {
                ReceiptClaimResult::Conflict
            } else if let Some(payload) = row
                .try_get::<Option<Vec<u8>>, _>("result_payload")
                .map_err(|_| PortError::Internal)?
            {
                ReceiptClaimResult::Replay(payload)
            } else {
                let old_lease: DateTime<Utc> = row
                    .try_get("lease_until")
                    .map_err(|_| PortError::Internal)?;
                if old_lease > Utc::now() {
                    ReceiptClaimResult::InProgress
                } else {
                    sqlx::query("UPDATE payment_command_receipts SET claim_token=$4,lease_until=$5 WHERE scope=$1 AND command=$2 AND key_digest=$3").bind(&claim.scope).bind(claim.command).bind(&claim.key_digest).bind(token).bind(lease).execute(&mut *tx).await.map_err(map_sql)?;
                    ReceiptClaimResult::Acquired {
                        token: token.to_string(),
                    }
                }
            }
        } else {
            sqlx::query("INSERT INTO payment_command_receipts(scope,command,key_digest,fingerprint,claim_token,lease_until) VALUES($1,$2,$3,$4,$5,$6)").bind(&claim.scope).bind(claim.command).bind(&claim.key_digest).bind(&claim.fingerprint).bind(token).bind(lease).execute(&mut *tx).await.map_err(map_sql)?;
            ReceiptClaimResult::Acquired {
                token: token.to_string(),
            }
        };
        tx.commit().await.map_err(map_sql)?;
        Ok(outcome)
    }
    async fn complete(&self, token: &str, result: &[u8]) -> Result<(), PortError> {
        let token = Uuid::parse_str(token).map_err(|_| PortError::Internal)?;
        let affected = sqlx::query("UPDATE payment_command_receipts SET result_payload=$2,completed_at=now() WHERE claim_token=$1 AND result_payload IS NULL").bind(token).bind(result).execute(&self.pool).await.map_err(map_sql)?.rows_affected();
        if affected == 1 {
            Ok(())
        } else {
            Err(PortError::Conflict)
        }
    }
    async fn release(&self, token: &str) -> Result<(), PortError> {
        let token = Uuid::parse_str(token).map_err(|_| PortError::Internal)?;
        sqlx::query("UPDATE payment_command_receipts SET lease_until=now() WHERE claim_token=$1 AND result_payload IS NULL").bind(token).execute(&self.pool).await.map_err(map_sql)?;
        Ok(())
    }
}

#[async_trait]
impl CallbackInbox for PostgresPayments {
    async fn claim(
        &self,
        provider: &str,
        event_ref_digest: &str,
        fingerprint: &str,
        lease_until_millis: u64,
    ) -> Result<CallbackClaim, PortError> {
        let lease = DateTime::<Utc>::from_timestamp_millis(
            i64::try_from(lease_until_millis).map_err(|_| PortError::Internal)?,
        )
        .ok_or(PortError::Internal)?;
        let mut tx = self.pool.begin().await.map_err(map_sql)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("callback:{provider}:{event_ref_digest}"))
            .execute(&mut *tx)
            .await
            .map_err(map_sql)?;
        let row = sqlx::query("SELECT fingerprint,lease_until,completed_at FROM payment_callback_inbox WHERE provider=$1 AND event_ref_digest=$2 FOR UPDATE")
            .bind(provider).bind(event_ref_digest).fetch_optional(&mut *tx).await.map_err(map_sql)?;
        let outcome = if let Some(row) = row {
            let stored: String = row
                .try_get("fingerprint")
                .map_err(|_| PortError::Internal)?;
            let completed: Option<DateTime<Utc>> = row
                .try_get("completed_at")
                .map_err(|_| PortError::Internal)?;
            let old_lease: DateTime<Utc> = row
                .try_get("lease_until")
                .map_err(|_| PortError::Internal)?;
            if stored != fingerprint {
                CallbackClaim::Conflict
            } else if completed.is_some() {
                CallbackClaim::Replay
            } else if old_lease > Utc::now() {
                CallbackClaim::InProgress
            } else {
                sqlx::query("UPDATE payment_callback_inbox SET lease_until=$3 WHERE provider=$1 AND event_ref_digest=$2")
                    .bind(provider).bind(event_ref_digest).bind(lease).execute(&mut *tx).await.map_err(map_sql)?;
                CallbackClaim::Acquired
            }
        } else {
            sqlx::query("INSERT INTO payment_callback_inbox(provider,event_ref_digest,fingerprint,lease_until) VALUES($1,$2,$3,$4)")
                .bind(provider).bind(event_ref_digest).bind(fingerprint).bind(lease).execute(&mut *tx).await.map_err(map_sql)?;
            CallbackClaim::Acquired
        };
        tx.commit().await.map_err(map_sql)?;
        Ok(outcome)
    }

    async fn complete(&self, provider: &str, event_ref_digest: &str) -> Result<(), PortError> {
        sqlx::query("UPDATE payment_callback_inbox SET completed_at=now() WHERE provider=$1 AND event_ref_digest=$2")
            .bind(provider).bind(event_ref_digest).execute(&self.pool).await.map_err(map_sql)?;
        Ok(())
    }
}

async fn append_events(
    tx: &mut Transaction<'_, Postgres>,
    events: &[DomainEvent],
) -> Result<(), CommitConflict> {
    for event in events {
        let (event_id, event_type, value) = public_event(event);
        sqlx::query("INSERT INTO payment_outbox(event_id,event_type,payload) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(event_id).bind(event_type).bind(Json(value)).execute(&mut **tx).await.map_err(|_| CommitConflict::Version)?;
    }
    Ok(())
}

fn public_event(event: &DomainEvent) -> (String, &'static str, serde_json::Value) {
    let (event_id, event_type, version, occurred_at_millis, payload) = match event {
        DomainEvent::PaymentInstrumentReady {
            event_id,
            instrument_id,
            owner_id,
            version,
            occurred_at_millis,
        } => (
            event_id,
            "PaymentInstrumentReady",
            *version,
            *occurred_at_millis,
            serde_json::json!({"instrumentId": instrument_id, "userId": owner_id}),
        ),
        DomainEvent::PaymentInstrumentRevoked {
            event_id,
            instrument_id,
            owner_id,
            version,
            occurred_at_millis,
        } => (
            event_id,
            "PaymentInstrumentRevoked",
            *version,
            *occurred_at_millis,
            serde_json::json!({"instrumentId": instrument_id, "userId": owner_id}),
        ),
        DomainEvent::PaymentAttemptStarted {
            event_id,
            attempt_id,
            auction_id,
            settlement_round,
            candidate_id,
            amount,
            version,
            occurred_at_millis,
        } => (
            event_id,
            "PaymentAttemptStarted",
            *version,
            *occurred_at_millis,
            serde_json::json!({
                "paymentAttemptId": attempt_id,
                "auctionId": auction_id,
                "settlementRound": settlement_round.get(),
                "candidateId": candidate_id,
                "amount": {
                    "minorUnits": amount.minor_units(),
                    "currencyCode": amount.currency().to_string()
                }
            }),
        ),
        DomainEvent::PaymentRecoveryRequired {
            event_id,
            attempt_id,
            auction_id,
            settlement_round,
            version,
            occurred_at_millis,
        } => (
            event_id,
            "PaymentRecoveryRequired",
            *version,
            *occurred_at_millis,
            serde_json::json!({"paymentAttemptId": attempt_id, "auctionId": auction_id, "settlementRound": settlement_round.get()}),
        ),
        DomainEvent::PaymentSucceeded {
            event_id,
            attempt_id,
            auction_id,
            settlement_round,
            version,
            occurred_at_millis,
        } => (
            event_id,
            "PaymentSucceeded",
            *version,
            *occurred_at_millis,
            serde_json::json!({"paymentAttemptId": attempt_id, "auctionId": auction_id, "settlementRound": settlement_round.get()}),
        ),
        DomainEvent::PaymentExpired {
            event_id,
            attempt_id,
            auction_id,
            settlement_round,
            version,
            occurred_at_millis,
        } => (
            event_id,
            "PaymentExpired",
            *version,
            *occurred_at_millis,
            serde_json::json!({"paymentAttemptId": attempt_id, "auctionId": auction_id, "settlementRound": settlement_round.get()}),
        ),
    };
    (
        event_id.clone(),
        event_type,
        serde_json::json!({
            "eventId": event_id,
            "eventType": event_type,
            "schemaVersion": 1,
            "aggregateVersion": version,
            "occurredAtMillis": occurred_at_millis,
            "correlationId": event_id,
            "payload": payload
        }),
    )
}
fn to_i64(value: u64) -> Result<i64, CommitConflict> {
    i64::try_from(value).map_err(|_| CommitConflict::Version)
}
fn instrument_state(value: PaymentInstrumentState) -> &'static str {
    match value {
        PaymentInstrumentState::Pending => "pending",
        PaymentInstrumentState::Ready => "ready",
        PaymentInstrumentState::Revoked => "revoked",
    }
}
fn attempt_state(value: PaymentAttemptState) -> &'static str {
    match value {
        PaymentAttemptState::Pending => "pending",
        PaymentAttemptState::Processing => "processing",
        PaymentAttemptState::RecoveryRequired => "recovery_required",
        PaymentAttemptState::Succeeded => "succeeded",
        PaymentAttemptState::Expired => "expired",
    }
}
fn map_sql(error: sqlx::Error) -> PortError {
    match error {
        sqlx::Error::RowNotFound => PortError::NotFound,
        sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_) => PortError::Unavailable,
        _ => PortError::Internal,
    }
}
fn constraint(error: &sqlx::Error, name: &str) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.constraint() == Some(name))
}
