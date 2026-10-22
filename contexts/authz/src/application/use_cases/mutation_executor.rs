use std::{future::Future, sync::Arc};

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::application::{
    ApplicationError, CommandReceiptStore, MutationOperation, MutationResult, ReceiptClaim,
    ReceiptClaimRequest, ReceiptStoreError, WorkloadIdentity,
};
use crate::telemetry::{ReceiptOutcome, record_receipt};

const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;

pub struct MutationCommand {
    pub caller: WorkloadIdentity,
    pub idempotency_key: String,
    pub operation: MutationOperation,
    pub canonical_payload: Vec<u8>,
}

#[derive(Clone)]
pub struct MutationExecutor<S> {
    receipt_store: Arc<S>,
    fingerprint_key: [u8; 32],
}

impl<S> MutationExecutor<S>
where
    S: CommandReceiptStore,
{
    pub const fn new(receipt_store: Arc<S>, fingerprint_key: [u8; 32]) -> Self {
        Self {
            receipt_store,
            fingerprint_key,
        }
    }

    pub async fn execute<F, Fut>(
        &self,
        command: MutationCommand,
        effect: F,
    ) -> Result<MutationResult, ApplicationError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<MutationResult, ApplicationError>>,
    {
        validate_idempotency_key(&command.idempotency_key)?;
        let fingerprint = self.fingerprint(command.operation, &command.canonical_payload);
        let claim = match self
            .receipt_store
            .claim(ReceiptClaimRequest {
                caller: command.caller,
                idempotency_key: command.idempotency_key,
                operation: command.operation,
                fingerprint,
            })
            .await
        {
            Ok(claim) => claim,
            Err(error) => {
                record_receipt(command.operation, ReceiptOutcome::Failed);
                return Err(map_store_error(error));
            }
        };

        let lease = match claim {
            ReceiptClaim::Replay(result) => {
                record_receipt(command.operation, ReceiptOutcome::Replayed);
                return Ok(result);
            }
            ReceiptClaim::Conflict => {
                record_receipt(command.operation, ReceiptOutcome::Conflict);
                return Err(ApplicationError::IdempotencyConflict);
            }
            ReceiptClaim::InProgress => {
                record_receipt(command.operation, ReceiptOutcome::InProgress);
                return Err(ApplicationError::CommandInProgress);
            }
            ReceiptClaim::Acquired(lease) => {
                record_receipt(
                    command.operation,
                    if lease.recovered {
                        ReceiptOutcome::Recovered
                    } else {
                        ReceiptOutcome::Claimed
                    },
                );
                lease
            }
        };

        let result = match effect().await {
            Ok(result) => result,
            Err(error) => {
                record_receipt(command.operation, ReceiptOutcome::Failed);
                return Err(error);
            }
        };
        if let Err(error) = self.receipt_store.complete(&lease, &result).await {
            record_receipt(command.operation, ReceiptOutcome::Failed);
            return Err(map_store_error(error));
        }
        record_receipt(command.operation, ReceiptOutcome::Completed);
        Ok(result)
    }

    fn fingerprint(&self, operation: MutationOperation, payload: &[u8]) -> [u8; 32] {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.fingerprint_key)
            .expect("HMAC-SHA256 accepts a key of any size");
        update_length_prefixed(&mut mac, operation.as_str().as_bytes());
        update_length_prefixed(&mut mac, payload);
        mac.finalize().into_bytes().into()
    }
}

fn validate_idempotency_key(value: &str) -> Result<(), ApplicationError> {
    if value.is_empty()
        || value.len() > MAX_IDEMPOTENCY_KEY_BYTES
        || value.chars().any(char::is_control)
    {
        Err(ApplicationError::InvalidArgument {
            reason: "invalid_idempotency_key",
            safe_message: "idempotency key is invalid".to_owned(),
        })
    } else {
        Ok(())
    }
}

fn update_length_prefixed(mac: &mut Hmac<Sha256>, value: &[u8]) {
    mac.update(&(value.len() as u64).to_be_bytes());
    mac.update(value);
}

fn map_store_error(error: ReceiptStoreError) -> ApplicationError {
    match error {
        ReceiptStoreError::Unavailable => ApplicationError::DependencyUnavailable {
            dependency: "receipt_store",
        },
        ReceiptStoreError::InvalidLease | ReceiptStoreError::Internal => ApplicationError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Mutex,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;

    use crate::application::{
        PrincipalRegistered, ReceiptClaim, ReceiptClaimRequest, ReceiptLease, ReceiptStoreError,
    };

    use super::*;

    #[derive(Clone)]
    enum State {
        Pending {
            fingerprint: [u8; 32],
            lease_token: String,
            lease_until: u64,
            attempt: u32,
        },
        Complete {
            fingerprint: [u8; 32],
            result: MutationResult,
        },
    }

    #[derive(Default)]
    struct MemoryReceiptStore {
        now: AtomicU64,
        sequence: AtomicU64,
        entries: Mutex<HashMap<(WorkloadIdentity, String), State>>,
    }

    impl MemoryReceiptStore {
        fn advance_past_lease(&self) {
            self.now.fetch_add(11, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl CommandReceiptStore for MemoryReceiptStore {
        async fn claim(
            &self,
            request: ReceiptClaimRequest,
        ) -> Result<ReceiptClaim, ReceiptStoreError> {
            let key = (request.caller, request.idempotency_key.clone());
            let now = self.now.load(Ordering::SeqCst);
            let mut entries = self.entries.lock().unwrap();
            match entries.get(&key).cloned() {
                Some(State::Complete {
                    fingerprint,
                    result,
                }) => {
                    if fingerprint == request.fingerprint {
                        Ok(ReceiptClaim::Replay(result))
                    } else {
                        Ok(ReceiptClaim::Conflict)
                    }
                }
                Some(State::Pending {
                    fingerprint,
                    lease_until,
                    attempt,
                    ..
                }) => {
                    if fingerprint != request.fingerprint {
                        return Ok(ReceiptClaim::Conflict);
                    }
                    if lease_until > now {
                        return Ok(ReceiptClaim::InProgress);
                    }
                    let lease_token =
                        format!("lease-{}", self.sequence.fetch_add(1, Ordering::SeqCst));
                    let lease = ReceiptLease {
                        caller: request.caller,
                        idempotency_key: request.idempotency_key,
                        fingerprint,
                        lease_token: lease_token.clone(),
                        attempt: attempt + 1,
                        recovered: true,
                    };
                    entries.insert(
                        key,
                        State::Pending {
                            fingerprint,
                            lease_token,
                            lease_until: now + 10,
                            attempt: attempt + 1,
                        },
                    );
                    Ok(ReceiptClaim::Acquired(lease))
                }
                None => {
                    let lease_token =
                        format!("lease-{}", self.sequence.fetch_add(1, Ordering::SeqCst));
                    let lease = ReceiptLease {
                        caller: request.caller,
                        idempotency_key: request.idempotency_key,
                        fingerprint: request.fingerprint,
                        lease_token: lease_token.clone(),
                        attempt: 1,
                        recovered: false,
                    };
                    entries.insert(
                        key,
                        State::Pending {
                            fingerprint: request.fingerprint,
                            lease_token,
                            lease_until: now + 10,
                            attempt: 1,
                        },
                    );
                    Ok(ReceiptClaim::Acquired(lease))
                }
            }
        }

        async fn complete(
            &self,
            lease: &ReceiptLease,
            result: &MutationResult,
        ) -> Result<(), ReceiptStoreError> {
            let key = (lease.caller, lease.idempotency_key.clone());
            let mut entries = self.entries.lock().unwrap();
            match entries.get(&key) {
                Some(State::Pending {
                    fingerprint,
                    lease_token,
                    ..
                }) if fingerprint == &lease.fingerprint && lease_token == &lease.lease_token => {
                    entries.insert(
                        key,
                        State::Complete {
                            fingerprint: lease.fingerprint,
                            result: result.clone(),
                        },
                    );
                    Ok(())
                }
                _ => Err(ReceiptStoreError::InvalidLease),
            }
        }
    }

    fn command(payload: &[u8]) -> MutationCommand {
        MutationCommand {
            caller: WorkloadIdentity::Bff,
            idempotency_key: "command-1".to_owned(),
            operation: MutationOperation::RegisterPrincipal,
            canonical_payload: payload.to_vec(),
        }
    }

    fn result() -> MutationResult {
        MutationResult::PrincipalRegistered(PrincipalRegistered {
            principal_id: "u1_opaque".to_owned(),
            written_at_token: "rev-1".to_owned(),
        })
    }

    #[tokio::test]
    async fn exact_replay_returns_stored_result_without_second_effect() {
        let store = Arc::new(MemoryReceiptStore::default());
        let executor = MutationExecutor::new(store, [3; 32]);
        let calls = Arc::new(AtomicUsize::new(0));

        for _ in 0..2 {
            let calls = Arc::clone(&calls);
            assert_eq!(
                executor
                    .execute(command(b"same"), || async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(result())
                    })
                    .await,
                Ok(result())
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn changed_payload_conflicts_and_concurrent_duplicate_is_in_progress() {
        let store = Arc::new(MemoryReceiptStore::default());
        let executor = MutationExecutor::new(Arc::clone(&store), [3; 32]);
        let first = store
            .claim(ReceiptClaimRequest {
                caller: WorkloadIdentity::Bff,
                idempotency_key: "command-1".to_owned(),
                operation: MutationOperation::RegisterPrincipal,
                fingerprint: executor.fingerprint(MutationOperation::RegisterPrincipal, b"same"),
            })
            .await
            .unwrap();
        assert!(matches!(first, ReceiptClaim::Acquired(_)));

        assert_eq!(
            executor
                .execute(command(b"same"), || async { Ok(result()) })
                .await,
            Err(ApplicationError::CommandInProgress)
        );
        assert_eq!(
            executor
                .execute(command(b"different"), || async { Ok(result()) })
                .await,
            Err(ApplicationError::IdempotencyConflict)
        );
    }

    #[tokio::test]
    async fn expired_lease_is_recovered_only_by_the_same_fingerprint() {
        let store = Arc::new(MemoryReceiptStore::default());
        let executor = MutationExecutor::new(Arc::clone(&store), [3; 32]);
        let first_error = executor
            .execute(command(b"same"), || async {
                Err(ApplicationError::DependencyUnavailable {
                    dependency: "policy_engine",
                })
            })
            .await;
        assert!(matches!(
            first_error,
            Err(ApplicationError::DependencyUnavailable { .. })
        ));
        store.advance_past_lease();

        assert_eq!(
            executor
                .execute(command(b"different"), || async { Ok(result()) })
                .await,
            Err(ApplicationError::IdempotencyConflict)
        );
        assert_eq!(
            executor
                .execute(command(b"same"), || async { Ok(result()) })
                .await,
            Ok(result())
        );
    }

    #[tokio::test]
    async fn invalid_key_is_rejected_before_store_claim() {
        let store = Arc::new(MemoryReceiptStore::default());
        let executor = MutationExecutor::new(Arc::clone(&store), [3; 32]);
        let mut invalid = command(b"same");
        invalid.idempotency_key.clear();
        assert!(matches!(
            executor.execute(invalid, || async { Ok(result()) }).await,
            Err(ApplicationError::InvalidArgument { .. })
        ));
        assert!(store.entries.lock().unwrap().is_empty());
    }
}
