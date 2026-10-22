use std::{sync::Arc, time::Instant};

use crate::application::{
    ApplicationError, BulkCheckPermissionsInput, BulkPermissionResult, PermissionDecision,
    PolicyDecision, PolicyEngine, PolicyEngineError, validate_consistency_token,
    validate_permission_check,
};
use crate::telemetry::{PermissionOperation, PermissionOutcome, record_permission};

pub const MAX_BULK_CHECKS: usize = 100;

pub struct BulkCheckPermissionsUseCase<P> {
    policy_engine: Arc<P>,
}

impl<P> BulkCheckPermissionsUseCase<P>
where
    P: PolicyEngine,
{
    pub fn new(policy_engine: Arc<P>) -> Self {
        Self { policy_engine }
    }

    pub async fn execute(
        &self,
        input: BulkCheckPermissionsInput,
    ) -> Result<BulkPermissionResult, ApplicationError> {
        let started = Instant::now();
        let result = self.execute_inner(input).await;
        let outcome = match &result {
            Ok(result) => PermissionOutcome::from_decisions(&result.decisions),
            Err(error) => PermissionOutcome::from_error(error),
        };
        record_permission(PermissionOperation::BulkCheck, outcome, started.elapsed());
        result
    }

    async fn execute_inner(
        &self,
        input: BulkCheckPermissionsInput,
    ) -> Result<BulkPermissionResult, ApplicationError> {
        if input.checks.is_empty() {
            return Err(ApplicationError::InvalidArgument {
                reason: "empty_batch",
                safe_message: "at least one permission check is required".to_owned(),
            });
        }
        if input.checks.len() > MAX_BULK_CHECKS {
            return Err(ApplicationError::BatchTooLarge);
        }
        for check in &input.checks {
            validate_permission_check(check)?;
        }
        validate_consistency_token(input.consistency_token.as_deref())?;

        let expected_count = input.checks.len();
        let result = self
            .policy_engine
            .bulk_check_permissions(&input.checks, input.consistency_token.as_deref())
            .await
            .map_err(map_policy_error)?;
        if result.checked_at_token.is_empty() || result.decisions.len() != expected_count {
            return Err(ApplicationError::Internal);
        }
        let decisions = result
            .decisions
            .into_iter()
            .map(|decision| match decision {
                PolicyDecision::Allowed => Ok(PermissionDecision::Allowed),
                PolicyDecision::Denied => Ok(PermissionDecision::Denied),
                PolicyDecision::Unknown => Err(ApplicationError::Internal),
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(BulkPermissionResult {
            decisions,
            checked_at_token: result.checked_at_token,
        })
    }
}

fn map_policy_error(error: PolicyEngineError) -> ApplicationError {
    match error {
        PolicyEngineError::InvalidArgument => ApplicationError::InvalidArgument {
            reason: "invalid_consistency_token",
            safe_message: "consistency token is invalid".to_owned(),
        },
        PolicyEngineError::Timeout | PolicyEngineError::Unavailable => {
            ApplicationError::DependencyUnavailable {
                dependency: "policy_engine",
            }
        }
        PolicyEngineError::InvalidResponse => ApplicationError::Internal,
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
        PermissionCheck, PolicyBulkCheckResult, PolicyCheckResult, PrincipalReference,
        ResourceReference, WorkloadIdentity,
    };

    use super::*;

    struct FakePolicyEngine {
        calls: AtomicUsize,
        response: Mutex<Option<Result<PolicyBulkCheckResult, PolicyEngineError>>>,
        observed_checks: Mutex<Vec<PermissionCheck>>,
    }

    #[async_trait]
    impl PolicyEngine for FakePolicyEngine {
        async fn check_permission(
            &self,
            _check: &PermissionCheck,
            _consistency_token: Option<&str>,
        ) -> Result<PolicyCheckResult, PolicyEngineError> {
            Err(PolicyEngineError::InvalidResponse)
        }

        async fn bulk_check_permissions(
            &self,
            checks: &[PermissionCheck],
            _consistency_token: Option<&str>,
        ) -> Result<PolicyBulkCheckResult, PolicyEngineError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.observed_checks
                .lock()
                .unwrap()
                .extend_from_slice(checks);
            self.response.lock().unwrap().take().unwrap()
        }
    }

    fn engine(response: Result<PolicyBulkCheckResult, PolicyEngineError>) -> Arc<FakePolicyEngine> {
        Arc::new(FakePolicyEngine {
            calls: AtomicUsize::new(0),
            response: Mutex::new(Some(response)),
            observed_checks: Mutex::new(Vec::new()),
        })
    }

    fn check(id: &str, permission: &str) -> PermissionCheck {
        PermissionCheck {
            resource: ResourceReference {
                resource_type: "auction".to_owned(),
                resource_id: id.to_owned(),
            },
            permission: permission.to_owned(),
            principal: PrincipalReference {
                principal_type: "user".to_owned(),
                principal_id: "u1_viewer".to_owned(),
            },
        }
    }

    fn input(checks: Vec<PermissionCheck>) -> BulkCheckPermissionsInput {
        BulkCheckPermissionsInput {
            caller: WorkloadIdentity::Bff,
            checks,
            consistency_token: None,
        }
    }

    #[tokio::test]
    async fn mixed_decisions_preserve_input_order_and_one_revision() {
        let engine = engine(Ok(PolicyBulkCheckResult {
            decisions: vec![
                PolicyDecision::Denied,
                PolicyDecision::Allowed,
                PolicyDecision::Denied,
            ],
            checked_at_token: "rev-common".to_owned(),
        }));
        let use_case = BulkCheckPermissionsUseCase::new(Arc::clone(&engine));
        let checks = vec![
            check("auction-1", "place_bid"),
            check("auction-2", "view_personal_state"),
            check("auction-1", "place_bid"),
        ];

        let result = use_case.execute(input(checks.clone())).await.unwrap();
        assert_eq!(
            result.decisions,
            vec![
                PermissionDecision::Denied,
                PermissionDecision::Allowed,
                PermissionDecision::Denied,
            ]
        );
        assert_eq!(result.checked_at_token, "rev-common");
        assert_eq!(&*engine.observed_checks.lock().unwrap(), &checks);
        assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn empty_oversized_and_invalid_batches_never_call_policy() {
        let engine = engine(Err(PolicyEngineError::Unavailable));
        let use_case = BulkCheckPermissionsUseCase::new(Arc::clone(&engine));

        assert!(matches!(
            use_case.execute(input(vec![])).await,
            Err(ApplicationError::InvalidArgument { .. })
        ));
        assert_eq!(
            use_case
                .execute(input(vec![
                    check("auction-1", "place_bid");
                    MAX_BULK_CHECKS + 1
                ]))
                .await,
            Err(ApplicationError::BatchTooLarge)
        );
        assert!(matches!(
            use_case
                .execute(input(vec![check("auction-1", "unknown")]))
                .await,
            Err(ApplicationError::InvalidArgument { .. })
        ));
        assert_eq!(engine.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn partial_or_unknown_dependency_response_returns_no_result() {
        let cases = [
            vec![PolicyDecision::Allowed],
            vec![PolicyDecision::Allowed, PolicyDecision::Unknown],
        ];
        for decisions in cases {
            let engine = engine(Ok(PolicyBulkCheckResult {
                decisions,
                checked_at_token: "rev".to_owned(),
            }));
            let use_case = BulkCheckPermissionsUseCase::new(engine);
            let error = use_case
                .execute(input(vec![
                    check("auction-1", "place_bid"),
                    check("auction-2", "place_bid"),
                ]))
                .await
                .unwrap_err();
            assert_eq!(error.code(), crate::application::ErrorCode::Internal);
        }
    }
}
