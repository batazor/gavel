use std::sync::Arc;

use crate::application::{
    ApplicationError, CommandReceiptStore, MutationOperation, MutationResult, PolicyEngine,
    PolicyEngineError, Relationship, RelationshipsWritten, WorkloadPolicy, WriteRelationshipsInput,
    validate_relationship,
};

use super::{MutationCommand, MutationExecutor};
use crate::telemetry::{RelationshipMutationKind, record_relationship_mutations};

pub const MAX_RELATIONSHIP_MUTATIONS: usize = 100;

pub struct WriteRelationshipsUseCase<P, S> {
    policy_engine: Arc<P>,
    executor: MutationExecutor<S>,
    workload_policy: WorkloadPolicy,
}

impl<P, S> WriteRelationshipsUseCase<P, S>
where
    P: PolicyEngine,
    S: CommandReceiptStore,
{
    pub fn new(policy_engine: Arc<P>, receipt_store: Arc<S>, fingerprint_key: [u8; 32]) -> Self {
        Self {
            policy_engine,
            executor: MutationExecutor::new(receipt_store, fingerprint_key),
            workload_policy: WorkloadPolicy,
        }
    }

    pub async fn execute(
        &self,
        input: WriteRelationshipsInput,
    ) -> Result<RelationshipsWritten, ApplicationError> {
        let (updates, deletions) =
            self.validate_and_canonicalize(input.caller, input.updates, input.deletions)?;
        let canonical_payload = canonical_payload(&updates, &deletions);
        let update_count = updates.len();
        let deletion_count = deletions.len();
        let policy_engine = Arc::clone(&self.policy_engine);
        let result = self
            .executor
            .execute(
                MutationCommand {
                    caller: input.caller,
                    idempotency_key: input.idempotency_key,
                    operation: MutationOperation::WriteRelationships,
                    canonical_payload,
                },
                || async move {
                    let written = policy_engine
                        .write_relationships(&updates, &deletions)
                        .await
                        .map_err(map_policy_error)?;
                    if written.written_at_token.is_empty() {
                        return Err(ApplicationError::Internal);
                    }
                    record_relationship_mutations(RelationshipMutationKind::Touch, update_count);
                    record_relationship_mutations(RelationshipMutationKind::Delete, deletion_count);
                    Ok(MutationResult::RelationshipsWritten(RelationshipsWritten {
                        written_at_token: written.written_at_token,
                    }))
                },
            )
            .await?;

        match result {
            MutationResult::RelationshipsWritten(result) => Ok(result),
            MutationResult::PrincipalRegistered(_) => Err(ApplicationError::Internal),
        }
    }

    fn validate_and_canonicalize(
        &self,
        caller: crate::application::WorkloadIdentity,
        mut updates: Vec<Relationship>,
        mut deletions: Vec<Relationship>,
    ) -> Result<(Vec<Relationship>, Vec<Relationship>), ApplicationError> {
        let count = updates.len().saturating_add(deletions.len());
        if count == 0 {
            return Err(ApplicationError::InvalidArgument {
                reason: "empty_relationship_batch",
                safe_message: "at least one relationship mutation is required".to_owned(),
            });
        }
        if count > MAX_RELATIONSHIP_MUTATIONS {
            return Err(ApplicationError::BatchTooLarge);
        }
        for relationship in updates.iter().chain(&deletions) {
            validate_relationship(relationship)?;
            self.workload_policy
                .authorize_relationship(caller, relationship)?;
        }
        updates.sort_by(|left, right| tuple_key(left).cmp(&tuple_key(right)));
        updates.dedup();
        deletions.sort_by(|left, right| tuple_key(left).cmp(&tuple_key(right)));
        deletions.dedup();
        if updates.iter().any(|relationship| {
            deletions
                .binary_search_by(|candidate| tuple_key(candidate).cmp(&tuple_key(relationship)))
                .is_ok()
        }) {
            return Err(ApplicationError::InvalidArgument {
                reason: "overlapping_relationship_mutation",
                safe_message: "a relationship cannot be updated and deleted together".to_owned(),
            });
        }
        Ok((updates, deletions))
    }
}

fn tuple_key(relationship: &Relationship) -> (&str, &str, &str, &str, &str) {
    (
        &relationship.resource.resource_type,
        &relationship.resource.resource_id,
        &relationship.relation,
        &relationship.subject.principal_type,
        &relationship.subject.principal_id,
    )
}

fn canonical_payload(updates: &[Relationship], deletions: &[Relationship]) -> Vec<u8> {
    let mut output = Vec::new();
    for (kind, relationships) in [(b'T', updates), (b'D', deletions)] {
        for relationship in relationships {
            output.push(kind);
            for value in [
                relationship.resource.resource_type.as_str(),
                relationship.resource.resource_id.as_str(),
                relationship.relation.as_str(),
                relationship.subject.principal_type.as_str(),
                relationship.subject.principal_id.as_str(),
            ] {
                output.extend_from_slice(&(value.len() as u64).to_be_bytes());
                output.extend_from_slice(value.as_bytes());
            }
        }
    }
    output
}

fn map_policy_error(error: PolicyEngineError) -> ApplicationError {
    match error {
        PolicyEngineError::Timeout | PolicyEngineError::Unavailable => {
            ApplicationError::DependencyUnavailable {
                dependency: "policy_engine",
            }
        }
        PolicyEngineError::InvalidArgument | PolicyEngineError::InvalidResponse => {
            ApplicationError::Internal
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;

    use crate::application::{
        PermissionCheck, PolicyBulkCheckResult, PolicyCheckResult, PolicyWriteResult,
        PrincipalReference, ReceiptClaim, ReceiptClaimRequest, ReceiptLease, ReceiptStoreError,
        ResourceReference, WorkloadIdentity,
    };

    use super::*;

    #[derive(Default)]
    struct FakePolicy {
        calls: AtomicUsize,
        batches: Mutex<Vec<(Vec<Relationship>, Vec<Relationship>)>>,
    }

    #[async_trait]
    impl PolicyEngine for FakePolicy {
        async fn check_permission(
            &self,
            _: &PermissionCheck,
            _: Option<&str>,
        ) -> Result<PolicyCheckResult, PolicyEngineError> {
            unreachable!()
        }
        async fn bulk_check_permissions(
            &self,
            _: &[PermissionCheck],
            _: Option<&str>,
        ) -> Result<PolicyBulkCheckResult, PolicyEngineError> {
            unreachable!()
        }
        async fn write_relationships(
            &self,
            updates: &[Relationship],
            deletions: &[Relationship],
        ) -> Result<PolicyWriteResult, PolicyEngineError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.batches
                .lock()
                .unwrap()
                .push((updates.to_vec(), deletions.to_vec()));
            Ok(PolicyWriteResult {
                written_at_token: "rev-write".to_owned(),
            })
        }
    }

    #[derive(Default)]
    struct MemoryStore(Mutex<Option<([u8; 32], MutationResult)>>);

    #[async_trait]
    impl CommandReceiptStore for MemoryStore {
        async fn claim(
            &self,
            request: ReceiptClaimRequest,
        ) -> Result<ReceiptClaim, ReceiptStoreError> {
            match self.0.lock().unwrap().clone() {
                Some((fingerprint, result)) if fingerprint == request.fingerprint => {
                    Ok(ReceiptClaim::Replay(result))
                }
                Some(_) => Ok(ReceiptClaim::Conflict),
                None => Ok(ReceiptClaim::Acquired(ReceiptLease {
                    caller: request.caller,
                    idempotency_key: request.idempotency_key,
                    fingerprint: request.fingerprint,
                    lease_token: "lease".to_owned(),
                    attempt: 1,
                    recovered: false,
                })),
            }
        }
        async fn complete(
            &self,
            lease: &ReceiptLease,
            result: &MutationResult,
        ) -> Result<(), ReceiptStoreError> {
            *self.0.lock().unwrap() = Some((lease.fingerprint, result.clone()));
            Ok(())
        }
    }

    fn relation(resource_type: &str, id: &str, relation: &str, subject_type: &str) -> Relationship {
        Relationship {
            resource: ResourceReference {
                resource_type: resource_type.to_owned(),
                resource_id: id.to_owned(),
            },
            relation: relation.to_owned(),
            subject: PrincipalReference {
                principal_type: subject_type.to_owned(),
                principal_id: "subject-1".to_owned(),
            },
        }
    }

    fn input(
        updates: Vec<Relationship>,
        deletions: Vec<Relationship>,
        key: &str,
    ) -> WriteRelationshipsInput {
        WriteRelationshipsInput {
            caller: WorkloadIdentity::Auction,
            updates,
            deletions,
            idempotency_key: key.to_owned(),
        }
    }

    #[tokio::test]
    async fn owner_batch_is_canonical_atomic_and_replayed() {
        let policy = Arc::new(FakePolicy::default());
        let use_case = WriteRelationshipsUseCase::new(
            Arc::clone(&policy),
            Arc::new(MemoryStore::default()),
            [5; 32],
        );
        let platform = relation("auction", "auction-1", "platform", "platform");
        let participant = relation("auction", "auction-1", "participant", "user");
        let request = input(
            vec![participant.clone(), platform.clone(), participant],
            vec![],
            "write-1",
        );
        let first = use_case.execute(request).await.unwrap();
        let replay = use_case
            .execute(input(vec![platform.clone()], vec![], "write-1"))
            .await;
        assert_eq!(first.written_at_token, "rev-write");
        assert_eq!(replay, Err(ApplicationError::IdempotencyConflict));
        let exact = use_case
            .execute(input(
                vec![
                    platform.clone(),
                    relation("auction", "auction-1", "participant", "user"),
                ],
                vec![],
                "write-1",
            ))
            .await
            .unwrap();
        assert_eq!(exact, first);
        assert_eq!(policy.calls.load(Ordering::SeqCst), 1);
        let batches = policy.batches.lock().unwrap();
        assert_eq!(batches[0].0.len(), 2);
        assert!(batches[0].0[0].relation < batches[0].0[1].relation);
    }

    #[tokio::test]
    async fn absent_delete_is_a_successful_atomic_write() {
        let policy = Arc::new(FakePolicy::default());
        let use_case = WriteRelationshipsUseCase::new(
            Arc::clone(&policy),
            Arc::new(MemoryStore::default()),
            [5; 32],
        );
        let deleted = relation("auction", "auction-1", "participant", "user");
        assert!(
            use_case
                .execute(input(vec![], vec![deleted.clone()], "delete-1"))
                .await
                .is_ok()
        );
        assert_eq!(policy.batches.lock().unwrap()[0].1, vec![deleted]);
    }

    #[tokio::test]
    async fn invalid_scope_shape_overlap_and_size_reject_the_whole_batch() {
        let cases = [
            input(vec![], vec![], "empty"),
            input(
                vec![relation("payment_profile", "p-1", "owner", "user")],
                vec![],
                "cross-owner",
            ),
            input(
                vec![relation("auction", "a-1", "participant", "service_account")],
                vec![],
                "bad-subject",
            ),
            input(
                vec![relation("auction", "a-1", "participant", "user")],
                vec![relation("auction", "a-1", "participant", "user")],
                "overlap",
            ),
            input(
                vec![
                    relation("auction", "a-1", "participant", "user");
                    MAX_RELATIONSHIP_MUTATIONS + 1
                ],
                vec![],
                "large",
            ),
        ];
        for request in cases {
            let policy = Arc::new(FakePolicy::default());
            let use_case = WriteRelationshipsUseCase::new(
                Arc::clone(&policy),
                Arc::new(MemoryStore::default()),
                [5; 32],
            );
            assert!(use_case.execute(request).await.is_err());
            assert_eq!(policy.calls.load(Ordering::SeqCst), 0);
        }
    }
}
