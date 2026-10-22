use std::future::Future;

use hmac::{Hmac, Mac};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::Sha256;

use super::{
    ApplicationError, CommandReceiptClaim, CommandReceiptOutcome, CommandReceiptStore, PortError,
    ReceiptCompletion,
};

type HmacSha256 = Hmac<Sha256>;

pub struct CommandExecutor<'a, S> {
    receipts: &'a S,
    fingerprint_key: &'a [u8],
}

impl<'a, S> CommandExecutor<'a, S>
where
    S: CommandReceiptStore,
{
    pub const fn new(receipts: &'a S, fingerprint_key: &'a [u8]) -> Self {
        Self {
            receipts,
            fingerprint_key,
        }
    }

    pub async fn execute<P, R, F, Fut>(
        &self,
        command: &'static str,
        caller_scope: &str,
        idempotency_key: &str,
        payload: &P,
        effect: F,
    ) -> Result<R, ApplicationError>
    where
        P: Serialize,
        R: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<R, ApplicationError>>,
    {
        validate_key(idempotency_key)?;
        let fingerprint = self.fingerprint(command, caller_scope, payload)?;
        let claim_token = match self
            .receipts
            .claim(CommandReceiptClaim {
                command,
                caller_scope: caller_scope.to_owned(),
                idempotency_key: idempotency_key.to_owned(),
                fingerprint,
            })
            .await
            .map_err(map_store)?
        {
            CommandReceiptOutcome::Acquired { claim_token } => claim_token,
            CommandReceiptOutcome::Replay { result_json } => {
                return serde_json::from_slice(&result_json)
                    .map_err(|_| ApplicationError::Internal);
            }
            CommandReceiptOutcome::Conflict => return Err(ApplicationError::IdempotencyConflict),
            CommandReceiptOutcome::InProgress => return Err(ApplicationError::CommandInProgress),
        };

        let result = match effect().await {
            Ok(result) => result,
            Err(error) => {
                self.receipts
                    .release_retry(&claim_token)
                    .await
                    .map_err(map_store)?;
                return Err(error);
            }
        };
        let result_json = serde_json::to_vec(&result).map_err(|_| ApplicationError::Internal)?;
        match self
            .receipts
            .complete(&claim_token, &result_json)
            .await
            .map_err(map_store)?
        {
            ReceiptCompletion::Stored => Ok(result),
            ReceiptCompletion::LostClaim => Err(ApplicationError::CommandInProgress),
        }
    }

    fn fingerprint<P: Serialize>(
        &self,
        command: &str,
        caller_scope: &str,
        payload: &P,
    ) -> Result<String, ApplicationError> {
        let mut value = serde_json::to_value(payload).map_err(|_| ApplicationError::Internal)?;
        canonicalize(&mut value);
        let canonical = serde_json::to_vec(&value).map_err(|_| ApplicationError::Internal)?;
        let mut mac = HmacSha256::new_from_slice(self.fingerprint_key)
            .map_err(|_| ApplicationError::Internal)?;
        mac.update(command.as_bytes());
        mac.update(&[0]);
        mac.update(caller_scope.as_bytes());
        mac.update(&[0]);
        mac.update(&canonical);
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            mac.finalize().into_bytes(),
        ))
    }
}

fn canonicalize(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(canonicalize),
        Value::Object(object) => {
            let mut entries: Vec<_> = std::mem::take(object).into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            for (key, mut value) in entries {
                canonicalize(&mut value);
                object.insert(key, value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn validate_key(value: &str) -> Result<(), ApplicationError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_whitespace) {
        return Err(ApplicationError::InvalidArgument {
            reason: "invalid_idempotency_key",
            safe_message: "idempotency key is invalid".to_owned(),
        });
    }
    Ok(())
}

fn map_store(error: PortError) -> ApplicationError {
    match error {
        PortError::Timeout | PortError::Unavailable => ApplicationError::DependencyUnavailable {
            dependency: "catalog_database",
        },
        PortError::NotFound | PortError::Internal => ApplicationError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use serde::{Deserialize, Serialize};
    use tokio::sync::Notify;

    use super::*;

    #[derive(Clone)]
    struct StoredReceipt {
        fingerprint: String,
        claim_token: String,
        result_json: Option<Vec<u8>>,
    }

    #[derive(Default)]
    struct FakeReceipts {
        receipt: Mutex<Option<StoredReceipt>>,
    }

    impl FakeReceipts {
        fn result_json(&self) -> Vec<u8> {
            self.receipt
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .result_json
                .clone()
                .unwrap()
        }
    }

    #[async_trait]
    impl CommandReceiptStore for FakeReceipts {
        async fn claim(
            &self,
            claim: CommandReceiptClaim,
        ) -> Result<CommandReceiptOutcome, PortError> {
            let mut slot = self.receipt.lock().unwrap();
            if let Some(receipt) = slot.as_ref() {
                if receipt.fingerprint != claim.fingerprint {
                    return Ok(CommandReceiptOutcome::Conflict);
                }
                return Ok(match &receipt.result_json {
                    Some(result_json) => CommandReceiptOutcome::Replay {
                        result_json: result_json.clone(),
                    },
                    None => CommandReceiptOutcome::InProgress,
                });
            }
            let claim_token = "opaque-claim-1".to_owned();
            *slot = Some(StoredReceipt {
                fingerprint: claim.fingerprint,
                claim_token: claim_token.clone(),
                result_json: None,
            });
            Ok(CommandReceiptOutcome::Acquired { claim_token })
        }

        async fn complete(
            &self,
            claim_token: &str,
            result_json: &[u8],
        ) -> Result<ReceiptCompletion, PortError> {
            let mut slot = self.receipt.lock().unwrap();
            let Some(receipt) = slot.as_mut() else {
                return Ok(ReceiptCompletion::LostClaim);
            };
            if receipt.claim_token != claim_token {
                return Ok(ReceiptCompletion::LostClaim);
            }
            receipt.result_json = Some(result_json.to_vec());
            Ok(ReceiptCompletion::Stored)
        }

        async fn release_retry(&self, claim_token: &str) -> Result<(), PortError> {
            let mut slot = self.receipt.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|receipt| receipt.claim_token == claim_token)
            {
                *slot = None;
            }
            Ok(())
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct ResultValue {
        aggregate_id: String,
        version: u64,
    }

    #[tokio::test]
    async fn canonical_hmac_claim_replays_and_conflicts_without_storing_payload() {
        let receipts = FakeReceipts::default();
        let executor = CommandExecutor::new(&receipts, b"test-hmac-key-at-least-32-bytes!");
        let first_payload = serde_json::json!({"title":"private-title", "nested":{"b":2,"a":1}});
        let reordered = serde_json::json!({"nested":{"a":1,"b":2}, "title":"private-title"});
        let effects = AtomicUsize::new(0);
        let expected = ResultValue {
            aggregate_id: "lot-1".to_owned(),
            version: 1,
        };
        let result = executor
            .execute("list_lot", "seller-1", "key-1", &first_payload, || async {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok(expected.clone())
            })
            .await
            .unwrap();
        assert_eq!(result, expected);
        let replay = executor
            .execute("list_lot", "seller-1", "key-1", &reordered, || async {
                panic!("replay must not execute the effect");
                #[allow(unreachable_code)]
                Ok(expected.clone())
            })
            .await
            .unwrap();
        assert_eq!(replay, expected);
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        let stored = String::from_utf8(receipts.result_json()).unwrap();
        assert!(!stored.contains("private-title"));
        assert!(!stored.contains("test-hmac-key"));

        let changed = serde_json::json!({"title":"changed"});
        assert_eq!(
            executor
                .execute("list_lot", "seller-1", "key-1", &changed, || async {
                    Ok(expected.clone())
                })
                .await
                .unwrap_err(),
            ApplicationError::IdempotencyConflict
        );
    }

    #[tokio::test]
    async fn concurrent_claims_allow_one_effect_and_report_in_progress() {
        let receipts = FakeReceipts::default();
        let executor = CommandExecutor::new(&receipts, b"test-hmac-key-at-least-32-bytes!");
        let started = Notify::new();
        let release = Notify::new();
        let effects = AtomicUsize::new(0);
        let payload = serde_json::json!({"lot":"lot-1"});
        let first = async {
            executor
                .execute("update_lot", "seller-1", "key-1", &payload, || async {
                    effects.fetch_add(1, Ordering::SeqCst);
                    started.notify_one();
                    release.notified().await;
                    Ok(ResultValue {
                        aggregate_id: "lot-1".to_owned(),
                        version: 2,
                    })
                })
                .await
        };
        let second = async {
            started.notified().await;
            let result = executor
                .execute("update_lot", "seller-1", "key-1", &payload, || async {
                    effects.fetch_add(1, Ordering::SeqCst);
                    Ok(ResultValue {
                        aggregate_id: "lot-1".to_owned(),
                        version: 99,
                    })
                })
                .await;
            release.notify_one();
            result
        };
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap().version, 2);
        assert_eq!(second.unwrap_err(), ApplicationError::CommandInProgress);
        assert_eq!(effects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retryable_effect_releases_claim_for_optimistic_retry() {
        let receipts = FakeReceipts::default();
        let executor = CommandExecutor::new(&receipts, b"test-hmac-key-at-least-32-bytes!");
        let payload = serde_json::json!({"lot":"lot-1", "expectedVersion":1});
        let first: Result<ResultValue, _> = executor
            .execute("update_lot", "seller-1", "key-1", &payload, || async {
                Err(ApplicationError::DependencyUnavailable {
                    dependency: "catalog_database",
                })
            })
            .await;
        assert!(matches!(
            first,
            Err(ApplicationError::DependencyUnavailable { .. })
        ));
        let retried = executor
            .execute("update_lot", "seller-1", "key-1", &payload, || async {
                Ok(ResultValue {
                    aggregate_id: "lot-1".to_owned(),
                    version: 2,
                })
            })
            .await
            .unwrap();
        assert_eq!(retried.version, 2);
    }
}
