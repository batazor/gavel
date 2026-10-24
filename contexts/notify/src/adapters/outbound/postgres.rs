use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Row, Transaction, types::Json};

use crate::{
    application::{ClaimResult, DeliveryAttempt, NotificationRepository, PortError},
    domain::{Notification, NotificationStatus},
};

#[derive(Clone)]
pub struct PostgresNotifications {
    pool: PgPool,
}

impl PostgresNotifications {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }
}

#[async_trait]
impl NotificationRepository for PostgresNotifications {
    async fn claim(&self, notification: Notification) -> Result<ClaimResult, PortError> {
        let mut tx = self.pool.begin().await.map_err(map_sql)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(notification.id())
            .execute(&mut *tx)
            .await
            .map_err(map_sql)?;
        let existing = sqlx::query(
            "SELECT fingerprint, aggregate FROM notifications WHERE notification_id=$1 FOR UPDATE",
        )
        .bind(notification.id())
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sql)?;
        let outcome = if let Some(row) = existing {
            let fingerprint: String = row
                .try_get("fingerprint")
                .map_err(|_| PortError::Internal)?;
            if fingerprint != notification.fingerprint() {
                ClaimResult::Conflict
            } else {
                let Json(value): Json<Notification> =
                    row.try_get("aggregate").map_err(|_| PortError::Internal)?;
                ClaimResult::Replay(value)
            }
        } else {
            sqlx::query("INSERT INTO notifications(notification_id,recipient_id,template_ref,fingerprint,status,attempt_count,version,aggregate) VALUES($1,$2,$3,$4,'pending',0,1,$5)")
                .bind(notification.id()).bind(notification.recipient_id()).bind(notification.template().reference()).bind(notification.fingerprint()).bind(Json(&notification))
                .execute(&mut *tx).await.map_err(map_sql)?;
            ClaimResult::Created(notification)
        };
        tx.commit().await.map_err(map_sql)?;
        Ok(outcome)
    }

    async fn save_attempt(
        &self,
        notification: &Notification,
        attempt: DeliveryAttempt,
    ) -> Result<(), PortError> {
        let version_delta = if notification.status() == NotificationStatus::Pending {
            1
        } else {
            2
        };
        let expected = notification
            .version()
            .checked_sub(version_delta)
            .ok_or(PortError::Internal)?;
        let mut tx = self.pool.begin().await.map_err(map_sql)?;
        let updated = sqlx::query("UPDATE notifications SET status=$2,attempt_count=$3,version=$4,aggregate=$5,updated_at=now() WHERE notification_id=$1 AND version=$6")
            .bind(notification.id()).bind(notification.status().as_db()).bind(i32::try_from(notification.attempts()).map_err(|_| PortError::Internal)?)
            .bind(i64::try_from(notification.version()).map_err(|_| PortError::Internal)?).bind(Json(notification)).bind(i64::try_from(expected).map_err(|_| PortError::Internal)?)
            .execute(&mut *tx).await.map_err(map_sql)?.rows_affected();
        if updated != 1 {
            return Err(PortError::Conflict);
        }
        sqlx::query("INSERT INTO notification_delivery_attempts(notification_id,attempt_number,outcome,safe_detail) VALUES($1,$2,$3,$4)")
            .bind(notification.id()).bind(i32::try_from(attempt.number).map_err(|_| PortError::Internal)?).bind(attempt.outcome).bind(attempt.safe_detail)
            .execute(&mut *tx).await.map_err(map_sql)?;
        if notification.status() != NotificationStatus::Pending {
            append_terminal_event(&mut tx, notification).await?;
        }
        tx.commit().await.map_err(map_sql)
    }

    async fn get(&self, notification_id: &str) -> Result<Option<Notification>, PortError> {
        sqlx::query_scalar::<_, Json<Notification>>(
            "SELECT aggregate FROM notifications WHERE notification_id=$1",
        )
        .bind(notification_id)
        .fetch_optional(&self.pool)
        .await
        .map(|value| value.map(|Json(value)| value))
        .map_err(map_sql)
    }
}

async fn append_terminal_event(
    tx: &mut Transaction<'_, Postgres>,
    notification: &Notification,
) -> Result<(), PortError> {
    let event_type = match notification.status() {
        NotificationStatus::Sent => "NotificationSent",
        NotificationStatus::Failed => "NotificationFailed",
        NotificationStatus::Pending => return Ok(()),
    };
    let event_id = format!("{}:{}", notification.id(), notification.version());
    let payload = serde_json::json!({"notification_id":notification.id(),"template":notification.template().reference(),"status":notification.status(),"version":notification.version()});
    sqlx::query("INSERT INTO notification_outbox(event_id,event_type,payload) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(event_id).bind(event_type).bind(payload).execute(&mut **tx).await.map_err(map_sql)?;
    Ok(())
}

fn map_sql(error: sqlx::Error) -> PortError {
    if let sqlx::Error::Database(db) = &error {
        if db.is_unique_violation() {
            return PortError::Conflict;
        }
    }
    if matches!(
        error,
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed
    ) {
        PortError::Unavailable
    } else {
        PortError::Internal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{NotificationCommand, Template};
    use std::collections::BTreeMap;

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL; exercised by the local MVP PostgreSQL path"]
    async fn claim_replays_exact_command_and_conflicts_on_mismatch(pool: PgPool) {
        let repository = PostgresNotifications::new(pool);
        let command = NotificationCommand {
            notification_id: "n-1".into(),
            recipient_id: "one@example.test".into(),
            template: Template::WinnerConfirmedV1,
            data: BTreeMap::new(),
            idempotency_key: "key".into(),
        };
        let first = Notification::pending(&command, "fingerprint-a".into());
        assert!(matches!(
            repository.claim(first.clone()).await.unwrap(),
            ClaimResult::Created(_)
        ));
        assert!(matches!(
            repository.claim(first).await.unwrap(),
            ClaimResult::Replay(_)
        ));
        let changed = Notification::pending(&command, "fingerprint-b".into());
        assert_eq!(
            repository.claim(changed).await.unwrap(),
            ClaimResult::Conflict
        );
    }
}
