use async_trait::async_trait;
use gavel_payments::application::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct FixedClock;
impl Clock for FixedClock {
    fn now_millis(&self) -> u64 {
        1_000
    }
}

#[derive(Default)]
struct MemoryReceipts {
    rows: Mutex<HashMap<(String, &'static str, String), Row>>,
}
struct Row {
    fingerprint: String,
    token: String,
    result: Option<Vec<u8>>,
}

#[async_trait]
impl ReceiptStore for MemoryReceipts {
    async fn claim(&self, claim: ReceiptClaim) -> Result<ReceiptClaimResult, PortError> {
        let mut rows = self.rows.lock().unwrap();
        let key = (claim.scope, claim.command, claim.key_digest);
        if let Some(row) = rows.get(&key) {
            if row.fingerprint != claim.fingerprint {
                return Ok(ReceiptClaimResult::Conflict);
            }
            return Ok(match &row.result {
                Some(value) => ReceiptClaimResult::Replay(value.clone()),
                None => ReceiptClaimResult::InProgress,
            });
        }
        let token = format!("claim-{}", rows.len() + 1);
        rows.insert(
            key,
            Row {
                fingerprint: claim.fingerprint,
                token: token.clone(),
                result: None,
            },
        );
        Ok(ReceiptClaimResult::Acquired { token })
    }
    async fn complete(&self, token: &str, result: &[u8]) -> Result<(), PortError> {
        let mut rows = self.rows.lock().unwrap();
        let row = rows
            .values_mut()
            .find(|row| row.token == token)
            .ok_or(PortError::NotFound)?;
        row.result = Some(result.to_vec());
        Ok(())
    }
    async fn release(&self, _token: &str) -> Result<(), PortError> {
        Ok(())
    }
}

#[tokio::test]
async fn concurrent_claim_has_one_executor_then_stable_replay_and_conflict() {
    let store = Arc::new(MemoryReceipts::default());
    let executor = Arc::new(ReceiptExecutor::new(
        store.clone(),
        Arc::new(FixedClock),
        CommandFingerprint::new([7_u8; 32]).unwrap(),
        30_000,
    ));
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let executor = executor.clone();
        tasks.push(tokio::spawn(async move {
            executor
                .claim::<_, String>(
                    "scope",
                    "command",
                    "raw-secret-key",
                    &serde_json::json!({"value": 1}),
                )
                .await
        }));
    }
    let mut token = None;
    for task in tasks {
        match task.await.unwrap() {
            Ok(ReceiptDecision::Execute { token: value }) => {
                assert!(token.replace(value).is_none());
            }
            Err(ApplicationError::InProgress) => {}
            other => panic!("unexpected claim: {other:?}"),
        }
    }
    let token = token.unwrap();
    executor
        .complete(&token, &"safe-result".to_owned())
        .await
        .unwrap();
    assert_eq!(
        executor
            .claim::<_, String>(
                "scope",
                "command",
                "raw-secret-key",
                &serde_json::json!({"value": 1})
            )
            .await
            .unwrap(),
        ReceiptDecision::Replay("safe-result".to_owned())
    );
    assert_eq!(
        executor
            .claim::<_, String>(
                "scope",
                "command",
                "raw-secret-key",
                &serde_json::json!({"value": 2})
            )
            .await
            .unwrap_err(),
        ApplicationError::IdempotencyConflict
    );
    let persisted = format!(
        "{:?}",
        store.rows.lock().unwrap().keys().collect::<Vec<_>>()
    );
    assert!(!persisted.contains("raw-secret-key"));
    assert!(!persisted.contains("\"value\""));
}
