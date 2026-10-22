use std::sync::Arc;

use crate::application::{
    ApplicationError, CommandReceiptStore, MutationOperation, MutationResult, PolicyEngine,
    PolicyEngineError, PrincipalDeriver, PrincipalReference, PrincipalRegistered,
    RegisterPrincipalInput, Relationship, ResourceReference, WorkloadPolicy,
};

use super::{MutationCommand, MutationExecutor};
use crate::telemetry::{RelationshipMutationKind, record_relationship_mutations};

pub struct RegisterPrincipalUseCase<P, S> {
    policy_engine: Arc<P>,
    executor: MutationExecutor<S>,
    principal_deriver: PrincipalDeriver,
    workload_policy: WorkloadPolicy,
}

impl<P, S> RegisterPrincipalUseCase<P, S>
where
    P: PolicyEngine,
    S: CommandReceiptStore,
{
    pub fn new(
        policy_engine: Arc<P>,
        receipt_store: Arc<S>,
        principal_key: [u8; 32],
        fingerprint_key: [u8; 32],
    ) -> Self {
        Self {
            policy_engine,
            executor: MutationExecutor::new(receipt_store, fingerprint_key),
            principal_deriver: PrincipalDeriver::new(principal_key),
            workload_policy: WorkloadPolicy,
        }
    }

    pub async fn execute(
        &self,
        input: RegisterPrincipalInput,
    ) -> Result<PrincipalRegistered, ApplicationError> {
        self.workload_policy
            .authorize_principal_registration(input.caller)?;
        let derived = self
            .principal_deriver
            .derive(&input.issuer, &input.subject)?;
        let principal_id = derived.principal_id;
        let canonical_payload = principal_id.as_bytes().to_vec();
        let relationship = Relationship {
            resource: ResourceReference {
                resource_type: "platform".to_owned(),
                resource_id: "gavel".to_owned(),
            },
            relation: "member".to_owned(),
            subject: PrincipalReference {
                principal_type: "user".to_owned(),
                principal_id: principal_id.clone(),
            },
        };
        let policy_engine = Arc::clone(&self.policy_engine);
        let result = self
            .executor
            .execute(
                MutationCommand {
                    caller: input.caller,
                    idempotency_key: input.idempotency_key,
                    operation: MutationOperation::RegisterPrincipal,
                    canonical_payload,
                },
                || async move {
                    let written = policy_engine
                        .write_relationships(&[relationship], &[])
                        .await
                        .map_err(map_policy_error)?;
                    if written.written_at_token.is_empty() {
                        return Err(ApplicationError::Internal);
                    }
                    record_relationship_mutations(RelationshipMutationKind::Touch, 1);
                    Ok(MutationResult::PrincipalRegistered(PrincipalRegistered {
                        principal_id,
                        written_at_token: written.written_at_token,
                    }))
                },
            )
            .await?;

        match result {
            MutationResult::PrincipalRegistered(result) => Ok(result),
            MutationResult::RelationshipsWritten(_) => Err(ApplicationError::Internal),
        }
    }
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
        PermissionCheck, PolicyBulkCheckResult, PolicyCheckResult, PolicyWriteResult, ReceiptClaim,
        ReceiptClaimRequest, ReceiptLease, ReceiptStoreError, WorkloadIdentity,
    };

    use super::*;

    struct FakePolicyEngine {
        writes: AtomicUsize,
        failure: Option<PolicyEngineError>,
        observed: Mutex<Vec<Relationship>>,
    }

    #[async_trait]
    impl PolicyEngine for FakePolicyEngine {
        async fn check_permission(
            &self,
            _check: &PermissionCheck,
            _consistency_token: Option<&str>,
        ) -> Result<PolicyCheckResult, PolicyEngineError> {
            unreachable!()
        }

        async fn bulk_check_permissions(
            &self,
            _checks: &[PermissionCheck],
            _consistency_token: Option<&str>,
        ) -> Result<PolicyBulkCheckResult, PolicyEngineError> {
            unreachable!()
        }

        async fn write_relationships(
            &self,
            updates: &[Relationship],
            deletions: &[Relationship],
        ) -> Result<PolicyWriteResult, PolicyEngineError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            assert!(deletions.is_empty());
            self.observed.lock().unwrap().extend_from_slice(updates);
            if let Some(error) = self.failure {
                Err(error)
            } else {
                Ok(PolicyWriteResult {
                    written_at_token: "revision-7".to_owned(),
                })
            }
        }
    }

    #[derive(Default)]
    struct MemoryStore {
        entry: Mutex<Option<([u8; 32], MutationResult)>>,
    }

    #[async_trait]
    impl CommandReceiptStore for MemoryStore {
        async fn claim(
            &self,
            request: ReceiptClaimRequest,
        ) -> Result<ReceiptClaim, ReceiptStoreError> {
            match self.entry.lock().unwrap().clone() {
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
            *self.entry.lock().unwrap() = Some((lease.fingerprint, result.clone()));
            Ok(())
        }
    }

    fn policy(failure: Option<PolicyEngineError>) -> Arc<FakePolicyEngine> {
        Arc::new(FakePolicyEngine {
            writes: AtomicUsize::new(0),
            failure,
            observed: Mutex::new(Vec::new()),
        })
    }

    fn input(caller: WorkloadIdentity) -> RegisterPrincipalInput {
        RegisterPrincipalInput {
            caller,
            issuer: "https://identity.example.test/oidc".to_owned(),
            subject: "external-user-42".to_owned(),
            idempotency_key: "login-1".to_owned(),
        }
    }

    #[tokio::test]
    async fn registers_stable_opaque_member_and_replays_without_second_write() {
        let policy = policy(None);
        let use_case = RegisterPrincipalUseCase::new(
            Arc::clone(&policy),
            Arc::new(MemoryStore::default()),
            [7; 32],
            [9; 32],
        );

        let first = use_case
            .execute(input(WorkloadIdentity::Bff))
            .await
            .unwrap();
        let replay = use_case
            .execute(input(WorkloadIdentity::Bff))
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert!(first.principal_id.starts_with("u1_"));
        assert_eq!(first.written_at_token, "revision-7");
        assert_eq!(policy.writes.load(Ordering::SeqCst), 1);
        let relationships = policy.observed.lock().unwrap();
        assert_eq!(relationships[0].resource.resource_type, "platform");
        assert_eq!(relationships[0].resource.resource_id, "gavel");
        assert_eq!(relationships[0].relation, "member");
        assert_eq!(relationships[0].subject.principal_id, first.principal_id);
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains("external-user-42"));
        assert!(!serialized.contains("identity.example"));
    }

    #[tokio::test]
    async fn non_bff_callers_are_rejected_before_policy_write() {
        for caller in [
            WorkloadIdentity::Auction,
            WorkloadIdentity::Catalog,
            WorkloadIdentity::Payments,
            WorkloadIdentity::Notify,
            WorkloadIdentity::Processes,
        ] {
            let policy = policy(None);
            let use_case = RegisterPrincipalUseCase::new(
                Arc::clone(&policy),
                Arc::new(MemoryStore::default()),
                [7; 32],
                [9; 32],
            );
            assert_eq!(
                use_case.execute(input(caller)).await,
                Err(ApplicationError::Forbidden)
            );
            assert_eq!(policy.writes.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn dependency_failure_is_retryable_and_not_completed() {
        let policy = policy(Some(PolicyEngineError::Timeout));
        let store = Arc::new(MemoryStore::default());
        let use_case = RegisterPrincipalUseCase::new(policy, Arc::clone(&store), [7; 32], [9; 32]);
        let error = use_case
            .execute(input(WorkloadIdentity::Bff))
            .await
            .unwrap_err();
        assert!(error.retryable());
        assert!(matches!(
            error,
            ApplicationError::DependencyUnavailable { .. }
        ));
        assert!(store.entry.lock().unwrap().is_none());
    }
}
