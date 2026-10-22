use std::time::Duration;

use async_trait::async_trait;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::application::{
    CommandReceiptStore, MutationResult, ReceiptClaim, ReceiptClaimRequest, ReceiptLease,
    ReceiptStoreError,
};

#[derive(Clone)]
pub struct PostgresReceiptStore {
    pool: PgPool,
    lease_millis: i64,
}

impl PostgresReceiptStore {
    pub fn new(pool: PgPool, lease_duration: Duration) -> Result<Self, ReceiptStoreError> {
        let lease_millis = i64::try_from(lease_duration.as_millis())
            .ok()
            .filter(|value| *value > 0)
            .ok_or(ReceiptStoreError::Internal)?;
        Ok(Self { pool, lease_millis })
    }
}

#[async_trait]
impl CommandReceiptStore for PostgresReceiptStore {
    async fn claim(&self, request: ReceiptClaimRequest) -> Result<ReceiptClaim, ReceiptStoreError> {
        let caller = request.caller.as_str();
        let mut transaction = self.pool.begin().await.map_err(map_sqlx_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1), hashtext($2))")
            .bind(caller)
            .bind(&request.idempotency_key)
            .execute(&mut *transaction)
            .await
            .map_err(map_sqlx_error)?;

        let existing = sqlx::query(
            "SELECT request_fingerprint, operation, status, attempts, result_payload \
             FROM authz_command_receipts WHERE caller_scope = $1 AND idempotency_key = $2 FOR UPDATE",
        )
        .bind(caller)
        .bind(&request.idempotency_key)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;

        if let Some(row) = existing {
            let fingerprint: Vec<u8> =
                row.try_get("request_fingerprint").map_err(map_sqlx_error)?;
            let operation: String = row.try_get("operation").map_err(map_sqlx_error)?;
            if fingerprint.as_slice() != request.fingerprint
                || operation != request.operation.as_str()
            {
                transaction.commit().await.map_err(map_sqlx_error)?;
                return Ok(ReceiptClaim::Conflict);
            }

            let status: String = row.try_get("status").map_err(map_sqlx_error)?;
            if status == "COMPLETED" {
                let payload: serde_json::Value =
                    row.try_get("result_payload").map_err(map_sqlx_error)?;
                let result =
                    serde_json::from_value(payload).map_err(|_| ReceiptStoreError::Internal)?;
                transaction.commit().await.map_err(map_sqlx_error)?;
                return Ok(ReceiptClaim::Replay(result));
            }
            if status != "PENDING" {
                return Err(ReceiptStoreError::Internal);
            }

            let lease_expired: bool = sqlx::query_scalar(
                "SELECT lease_expires_at <= clock_timestamp() FROM authz_command_receipts \
                 WHERE caller_scope = $1 AND idempotency_key = $2",
            )
            .bind(caller)
            .bind(&request.idempotency_key)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_sqlx_error)?;
            if !lease_expired {
                transaction.commit().await.map_err(map_sqlx_error)?;
                return Ok(ReceiptClaim::InProgress);
            }

            let attempt: i32 = row.try_get("attempts").map_err(map_sqlx_error)?;
            let lease_token = Uuid::now_v7();
            sqlx::query(
                "UPDATE authz_command_receipts \
                 SET lease_token = $3, lease_expires_at = clock_timestamp() + \
                     make_interval(secs => $4::double precision / 1000.0), \
                     attempts = attempts + 1, updated_at = clock_timestamp() \
                 WHERE caller_scope = $1 AND idempotency_key = $2",
            )
            .bind(caller)
            .bind(&request.idempotency_key)
            .bind(lease_token)
            .bind(self.lease_millis)
            .execute(&mut *transaction)
            .await
            .map_err(map_sqlx_error)?;
            transaction.commit().await.map_err(map_sqlx_error)?;
            return Ok(ReceiptClaim::Acquired(ReceiptLease {
                caller: request.caller,
                idempotency_key: request.idempotency_key,
                fingerprint: request.fingerprint,
                lease_token: lease_token.to_string(),
                attempt: u32::try_from(attempt + 1).map_err(|_| ReceiptStoreError::Internal)?,
                recovered: true,
            }));
        }

        let lease_token = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO authz_command_receipts \
             (caller_scope, idempotency_key, request_fingerprint, operation, status, \
              lease_token, lease_expires_at, attempts) \
             VALUES ($1, $2, $3, $4, 'PENDING', $5, \
                     clock_timestamp() + make_interval(secs => $6::double precision / 1000.0), 1)",
        )
        .bind(caller)
        .bind(&request.idempotency_key)
        .bind(request.fingerprint.as_slice())
        .bind(request.operation.as_str())
        .bind(lease_token)
        .bind(self.lease_millis)
        .execute(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;
        transaction.commit().await.map_err(map_sqlx_error)?;
        Ok(ReceiptClaim::Acquired(ReceiptLease {
            caller: request.caller,
            idempotency_key: request.idempotency_key,
            fingerprint: request.fingerprint,
            lease_token: lease_token.to_string(),
            attempt: 1,
            recovered: false,
        }))
    }

    async fn complete(
        &self,
        lease: &ReceiptLease,
        result: &MutationResult,
    ) -> Result<(), ReceiptStoreError> {
        let lease_token =
            Uuid::parse_str(&lease.lease_token).map_err(|_| ReceiptStoreError::InvalidLease)?;
        let payload = serde_json::to_value(result).map_err(|_| ReceiptStoreError::Internal)?;
        let mut transaction = self.pool.begin().await.map_err(map_sqlx_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1), hashtext($2))")
            .bind(lease.caller.as_str())
            .bind(&lease.idempotency_key)
            .execute(&mut *transaction)
            .await
            .map_err(map_sqlx_error)?;
        let changed = sqlx::query(
            "UPDATE authz_command_receipts \
             SET status = 'COMPLETED', lease_token = NULL, lease_expires_at = NULL, \
                 result_payload = $5, updated_at = clock_timestamp() \
             WHERE caller_scope = $1 AND idempotency_key = $2 AND status = 'PENDING' \
               AND request_fingerprint = $3 AND lease_token = $4",
        )
        .bind(lease.caller.as_str())
        .bind(&lease.idempotency_key)
        .bind(lease.fingerprint.as_slice())
        .bind(lease_token)
        .bind(payload)
        .execute(&mut *transaction)
        .await
        .map_err(map_sqlx_error)?;
        if changed.rows_affected() != 1 {
            transaction.rollback().await.map_err(map_sqlx_error)?;
            return Err(ReceiptStoreError::InvalidLease);
        }
        transaction.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }
}

fn map_sqlx_error(_: sqlx::Error) -> ReceiptStoreError {
    ReceiptStoreError::Unavailable
}
