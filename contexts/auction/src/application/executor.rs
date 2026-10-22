use std::{fmt::Write, future::Future, sync::Arc};

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::domain::{Auction, AuctionEvent, PublicEvent};
use crate::telemetry::{
    CommandOperation, CommandOutcome, IdempotencyOutcome, record_command, record_idempotency,
    record_retry,
};

use super::{
    ApplicationError, AuctionProjection, AuctionUnitOfWork, CommandContext, CommandIdentity,
    CommandLookup, CommandResult, CommitError, CommitOutcome, CommitRequest,
    ReconciliationOperation,
};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCommand {
    pub private_events: Vec<AuctionEvent>,
    pub public_events: Vec<PublicEvent>,
    pub result: CommandResult,
    pub projection: Option<AuctionProjection>,
    pub reconciliation: Vec<ReconciliationOperation>,
}

pub struct CommandExecutor<U> {
    unit_of_work: Arc<U>,
    max_attempts: usize,
    fingerprint_key: [u8; 32],
}

impl<U> Clone for CommandExecutor<U> {
    fn clone(&self) -> Self {
        Self {
            unit_of_work: Arc::clone(&self.unit_of_work),
            max_attempts: self.max_attempts,
            fingerprint_key: self.fingerprint_key,
        }
    }
}

impl<U> CommandExecutor<U>
where
    U: AuctionUnitOfWork,
{
    pub fn new(unit_of_work: Arc<U>, max_attempts: usize, fingerprint_key: [u8; 32]) -> Self {
        Self {
            unit_of_work,
            max_attempts: max_attempts.max(1),
            fingerprint_key,
        }
    }

    pub async fn execute<Authorize, AuthorizeFuture, Decide>(
        &self,
        context: &CommandContext,
        operation: &'static str,
        canonical_payload: &[u8],
        auction_id: &str,
        authorize: Authorize,
        mut decide: Decide,
    ) -> Result<CommandResult, ApplicationError>
    where
        Authorize: FnOnce() -> AuthorizeFuture,
        AuthorizeFuture: Future<Output = Result<(), ApplicationError>>,
        Decide: FnMut(&Auction) -> Result<PreparedCommand, ApplicationError>,
    {
        self.validate_context(context)?;
        let metric_operation = CommandOperation::from_name(operation);
        record_command(metric_operation, CommandOutcome::Started);
        let identity = self.command_identity(context, operation, canonical_payload);

        match self.unit_of_work.lookup_command(&identity).await {
            Ok(CommandLookup::Replay(result)) => {
                record_idempotency(IdempotencyOutcome::Hit);
                record_command(metric_operation, CommandOutcome::Replayed);
                return Ok(result);
            }
            Ok(CommandLookup::Conflict) => {
                record_idempotency(IdempotencyOutcome::Conflict);
                record_command(metric_operation, CommandOutcome::Rejected);
                return Err(ApplicationError::IdempotencyConflict);
            }
            Ok(CommandLookup::Missing) => record_idempotency(IdempotencyOutcome::Miss),
            Err(error) => {
                record_command(metric_operation, CommandOutcome::Failed);
                return Err(map_commit_error(error));
            }
        }

        if let Err(error) = authorize().await {
            record_command(metric_operation, CommandOutcome::Rejected);
            return Err(error);
        }

        for attempt in 0..self.max_attempts {
            let loaded = self
                .unit_of_work
                .load(auction_id)
                .await
                .map_err(map_commit_error)?;
            let (aggregate, expected_version) = match loaded {
                Some(loaded) => {
                    let mut aggregate: Auction = loaded.snapshot.unwrap_or_default();
                    for event in loaded.events {
                        aggregate.apply(event).map_err(ApplicationError::from)?;
                    }
                    (aggregate, loaded.version)
                }
                None => (Auction::default(), 0),
            };
            let prepared = decide(&aggregate)?;
            let result = prepared.result.clone();
            let request = CommitRequest {
                auction_id: auction_id.to_owned(),
                expected_version,
                private_events: prepared.private_events,
                public_events: prepared.public_events,
                command: Some((identity.clone(), result.clone())),
                projection: prepared.projection,
                reconciliation: prepared.reconciliation,
            };

            match self.unit_of_work.commit(request).await {
                Ok(CommitOutcome::Committed { .. }) => {
                    record_command(metric_operation, CommandOutcome::Succeeded);
                    return Ok(result);
                }
                Ok(CommitOutcome::Replayed { result }) => {
                    record_idempotency(IdempotencyOutcome::Hit);
                    record_command(metric_operation, CommandOutcome::Replayed);
                    return Ok(result);
                }
                Err(CommitError::VersionConflict) if attempt + 1 < self.max_attempts => {
                    record_retry(metric_operation);
                    continue;
                }
                Err(CommitError::VersionConflict) => {
                    record_command(metric_operation, CommandOutcome::Failed);
                    return Err(ApplicationError::Contention);
                }
                Err(error) => {
                    record_command(metric_operation, CommandOutcome::Failed);
                    return Err(map_commit_error(error));
                }
            }
        }

        record_command(metric_operation, CommandOutcome::Failed);
        Err(ApplicationError::Contention)
    }

    fn validate_context(&self, context: &CommandContext) -> Result<(), ApplicationError> {
        if context.principal.id.trim().is_empty() {
            return Err(ApplicationError::Unauthenticated);
        }
        if context.idempotency_key.trim().is_empty() {
            return Err(ApplicationError::InvalidArgument {
                reason: "missing_idempotency_key",
                safe_message: "idempotency key is required".to_owned(),
            });
        }
        Ok(())
    }

    fn command_identity(
        &self,
        context: &CommandContext,
        operation: &str,
        canonical_payload: &[u8],
    ) -> CommandIdentity {
        let principal_kind = match context.principal.kind {
            super::PrincipalKind::User => "user",
            super::PrincipalKind::ServiceAccount => "service_account",
        };
        let principal_scope = format!("{principal_kind}:{}", context.principal.id);
        let mut mac = HmacSha256::new_from_slice(&self.fingerprint_key)
            .expect("SHA-256 HMAC accepts a key of any size");
        for component in [
            operation.as_bytes(),
            principal_scope.as_bytes(),
            canonical_payload,
        ] {
            mac.update(&(component.len() as u64).to_be_bytes());
            mac.update(component);
        }
        let mut request_fingerprint = String::with_capacity(64);
        for byte in mac.finalize().into_bytes() {
            write!(&mut request_fingerprint, "{byte:02x}")
                .expect("writing into a String cannot fail");
        }

        CommandIdentity {
            principal_scope,
            idempotency_key: context.idempotency_key.clone(),
            request_fingerprint,
            operation: operation.to_owned(),
            correlation_id: context.correlation_id.clone(),
            causation_id: context.causation_id.clone(),
        }
    }
}

fn map_commit_error(error: CommitError) -> ApplicationError {
    match error {
        CommitError::VersionConflict => ApplicationError::Contention,
        CommitError::IdempotencyConflict => ApplicationError::IdempotencyConflict,
        CommitError::Unavailable => ApplicationError::DependencyUnavailable {
            dependency: "auction_db",
        },
        CommitError::InvalidRequest | CommitError::Internal => ApplicationError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::application::{
        CommandContext, Principal, PrincipalKind, testing::InMemoryAuctionStore,
    };

    fn context(key: &str) -> CommandContext {
        CommandContext {
            principal: Principal {
                kind: PrincipalKind::User,
                id: "user-1".to_owned(),
            },
            idempotency_key: key.to_owned(),
            correlation_id: "correlation-1".to_owned(),
            causation_id: "request-1".to_owned(),
        }
    }

    fn prepared() -> PreparedCommand {
        PreparedCommand {
            private_events: Vec::new(),
            public_events: Vec::new(),
            result: CommandResult::Opened { version: 0 },
            projection: None,
            reconciliation: Vec::new(),
        }
    }

    #[tokio::test]
    async fn replay_happens_before_authorization() {
        let store = Arc::new(InMemoryAuctionStore::default());
        let executor = CommandExecutor::new(store, 3, [7; 32]);
        let authorizations = AtomicUsize::new(0);

        for _ in 0..2 {
            let result = executor
                .execute(
                    &context("key-1"),
                    "place_bid",
                    b"canonical-secret-payload",
                    "auction-1",
                    || async {
                        authorizations.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                    |_| Ok(prepared()),
                )
                .await
                .unwrap();
            assert_eq!(result, CommandResult::Opened { version: 0 });
        }

        assert_eq!(authorizations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn changed_payload_conflicts_for_same_key() {
        let store = Arc::new(InMemoryAuctionStore::default());
        let executor = CommandExecutor::new(store, 3, [7; 32]);
        executor
            .execute(
                &context("key-1"),
                "place_bid",
                b"payload-a",
                "auction-1",
                || async { Ok(()) },
                |_| Ok(prepared()),
            )
            .await
            .unwrap();

        let result = executor
            .execute(
                &context("key-1"),
                "place_bid",
                b"payload-b",
                "auction-1",
                || async { Ok(()) },
                |_| Ok(prepared()),
            )
            .await;
        assert_eq!(result, Err(ApplicationError::IdempotencyConflict));
    }

    #[tokio::test]
    async fn optimistic_conflict_retries_then_succeeds() {
        let store = Arc::new(InMemoryAuctionStore::default());
        store.conflict_next_commits(2);
        let executor = CommandExecutor::new(store, 3, [7; 32]);
        let decisions = AtomicUsize::new(0);

        let result = executor
            .execute(
                &context("key-1"),
                "place_bid",
                b"payload",
                "auction-1",
                || async { Ok(()) },
                |_| {
                    decisions.fetch_add(1, Ordering::SeqCst);
                    Ok(prepared())
                },
            )
            .await;

        assert_eq!(result, Ok(CommandResult::Opened { version: 0 }));
        assert_eq!(decisions.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_budget_exhaustion_is_retryable_contention() {
        let store = Arc::new(InMemoryAuctionStore::default());
        store.conflict_next_commits(3);
        let executor = CommandExecutor::new(store, 3, [7; 32]);

        let result = executor
            .execute(
                &context("key-1"),
                "place_bid",
                b"payload",
                "auction-1",
                || async { Ok(()) },
                |_| Ok(prepared()),
            )
            .await;

        assert_eq!(result, Err(ApplicationError::Contention));
        assert!(result.unwrap_err().retryable());
    }

    #[tokio::test]
    async fn concurrent_duplicate_has_one_business_effect() {
        let store = Arc::new(InMemoryAuctionStore::default());
        let executor = CommandExecutor::new(store, 3, [7; 32]);
        let effects = AtomicUsize::new(0);
        let first_context = context("key-1");
        let second_context = context("key-1");
        let first = executor.execute(
            &first_context,
            "place_bid",
            b"payload",
            "auction-1",
            || async { Ok(()) },
            |_| {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok(prepared())
            },
        );
        let second = executor.execute(
            &second_context,
            "place_bid",
            b"payload",
            "auction-1",
            || async { Ok(()) },
            |_| {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok(prepared())
            },
        );

        let (first, second) = tokio::join!(first, second);
        assert_eq!(first, Ok(CommandResult::Opened { version: 0 }));
        assert_eq!(second, Ok(CommandResult::Opened { version: 0 }));
        assert_eq!(effects.load(Ordering::SeqCst), 1);
    }
}
