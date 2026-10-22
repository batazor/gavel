use std::{sync::Arc, time::Instant};

use crate::application::{
    ApplicationError, CheckPermissionInput, PermissionDecision, PermissionResult, PolicyDecision,
    PolicyEngine, PolicyEngineError, validate_consistency_token, validate_permission_check,
};
use crate::telemetry::{PermissionOperation, PermissionOutcome, record_permission};

pub struct CheckPermissionUseCase<P> {
    policy_engine: Arc<P>,
}

impl<P> CheckPermissionUseCase<P>
where
    P: PolicyEngine,
{
    pub fn new(policy_engine: Arc<P>) -> Self {
        Self { policy_engine }
    }

    pub async fn execute(
        &self,
        input: CheckPermissionInput,
    ) -> Result<PermissionResult, ApplicationError> {
        let started = Instant::now();
        let result = self.execute_inner(input).await;
        let outcome = match &result {
            Ok(result) => match result.decision {
                PermissionDecision::Allowed => PermissionOutcome::Allowed,
                PermissionDecision::Denied => PermissionOutcome::Denied,
            },
            Err(error) => PermissionOutcome::from_error(error),
        };
        record_permission(PermissionOperation::Check, outcome, started.elapsed());
        result
    }

    async fn execute_inner(
        &self,
        input: CheckPermissionInput,
    ) -> Result<PermissionResult, ApplicationError> {
        validate_permission_check(&input.check)?;
        validate_consistency_token(input.consistency_token.as_deref())?;

        let result = self
            .policy_engine
            .check_permission(&input.check, input.consistency_token.as_deref())
            .await
            .map_err(map_policy_error)?;
        if result.checked_at_token.is_empty() {
            return Err(ApplicationError::Internal);
        }
        let decision = match result.decision {
            PolicyDecision::Allowed => PermissionDecision::Allowed,
            PolicyDecision::Denied => PermissionDecision::Denied,
            PolicyDecision::Unknown => return Err(ApplicationError::Internal),
        };
        Ok(PermissionResult {
            decision,
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
    use std::{
        collections::VecDeque,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;

    use crate::application::{ErrorCode, PrincipalReference, ResourceReference, WorkloadIdentity};

    use super::*;

    struct FakePolicyEngine {
        calls: AtomicUsize,
        responses:
            Mutex<VecDeque<Result<crate::application::PolicyCheckResult, PolicyEngineError>>>,
        tokens: Mutex<Vec<Option<String>>>,
    }

    impl FakePolicyEngine {
        fn new(
            responses: impl IntoIterator<
                Item = Result<crate::application::PolicyCheckResult, PolicyEngineError>,
            >,
        ) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                responses: Mutex::new(responses.into_iter().collect()),
                tokens: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl PolicyEngine for FakePolicyEngine {
        async fn check_permission(
            &self,
            _check: &crate::application::PermissionCheck,
            consistency_token: Option<&str>,
        ) -> Result<crate::application::PolicyCheckResult, PolicyEngineError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.tokens
                .lock()
                .unwrap()
                .push(consistency_token.map(str::to_owned));
            self.responses.lock().unwrap().pop_front().unwrap()
        }

        async fn bulk_check_permissions(
            &self,
            _checks: &[crate::application::PermissionCheck],
            _consistency_token: Option<&str>,
        ) -> Result<crate::application::PolicyBulkCheckResult, PolicyEngineError> {
            Err(PolicyEngineError::InvalidResponse)
        }
    }

    fn input() -> CheckPermissionInput {
        CheckPermissionInput {
            caller: WorkloadIdentity::Auction,
            check: crate::application::PermissionCheck {
                resource: ResourceReference {
                    resource_type: "auction".to_owned(),
                    resource_id: "auction-1".to_owned(),
                },
                permission: "place_bid".to_owned(),
                principal: PrincipalReference {
                    principal_type: "user".to_owned(),
                    principal_id: "u1_bidder".to_owned(),
                },
            },
            consistency_token: None,
        }
    }

    fn result(decision: PolicyDecision, token: &str) -> crate::application::PolicyCheckResult {
        crate::application::PolicyCheckResult {
            decision,
            checked_at_token: token.to_owned(),
        }
    }

    #[tokio::test]
    async fn invalid_references_never_call_the_policy_engine() {
        let engine = Arc::new(FakePolicyEngine::new([]));
        let use_case = CheckPermissionUseCase::new(Arc::clone(&engine));
        let mut cases = vec![];

        let mut empty = input();
        empty.check.resource.resource_id.clear();
        cases.push(empty);
        let mut unknown_resource = input();
        unknown_resource.check.resource.resource_type = "mystery".to_owned();
        cases.push(unknown_resource);
        let mut unknown_permission = input();
        unknown_permission.check.permission = "destroy".to_owned();
        cases.push(unknown_permission);
        let mut unknown_principal = input();
        unknown_principal.check.principal.principal_type = "robot".to_owned();
        cases.push(unknown_principal);

        for input in cases {
            assert!(matches!(
                use_case.execute(input).await,
                Err(ApplicationError::InvalidArgument { .. })
            ));
        }
        assert_eq!(engine.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn maps_allow_deny_and_forwards_causal_token() {
        let engine = Arc::new(FakePolicyEngine::new([
            Ok(result(PolicyDecision::Allowed, "rev-2")),
            Ok(result(PolicyDecision::Denied, "rev-3")),
        ]));
        let use_case = CheckPermissionUseCase::new(Arc::clone(&engine));
        let mut causal = input();
        causal.consistency_token = Some("rev-1".to_owned());

        let allowed = use_case.execute(causal).await.unwrap();
        let denied = use_case.execute(input()).await.unwrap();
        assert_eq!(allowed.decision, PermissionDecision::Allowed);
        assert_eq!(allowed.checked_at_token, "rev-2");
        assert_eq!(denied.decision, PermissionDecision::Denied);
        assert_eq!(
            &*engine.tokens.lock().unwrap(),
            &[Some("rev-1".to_owned()), None]
        );
    }

    #[tokio::test]
    async fn malformed_token_is_rejected_locally_or_by_the_policy_engine() {
        let engine = Arc::new(FakePolicyEngine::new([Err(
            PolicyEngineError::InvalidArgument,
        )]));
        let use_case = CheckPermissionUseCase::new(Arc::clone(&engine));
        let mut empty_token = input();
        empty_token.consistency_token = Some(String::new());
        assert!(matches!(
            use_case.execute(empty_token).await,
            Err(ApplicationError::InvalidArgument { .. })
        ));
        assert_eq!(engine.calls.load(Ordering::SeqCst), 0);

        let mut opaque_but_bad = input();
        opaque_but_bad.consistency_token = Some("malformed".to_owned());
        assert!(matches!(
            use_case.execute(opaque_but_bad).await,
            Err(ApplicationError::InvalidArgument { .. })
        ));
        assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dependency_and_unknown_results_fail_closed() {
        let cases = [
            (
                Err(PolicyEngineError::Timeout),
                ErrorCode::DependencyUnavailable,
            ),
            (
                Err(PolicyEngineError::Unavailable),
                ErrorCode::DependencyUnavailable,
            ),
            (Err(PolicyEngineError::InvalidResponse), ErrorCode::Internal),
            (
                Ok(result(PolicyDecision::Unknown, "rev")),
                ErrorCode::Internal,
            ),
            (Ok(result(PolicyDecision::Allowed, "")), ErrorCode::Internal),
        ];

        for (response, expected) in cases {
            let use_case = CheckPermissionUseCase::new(Arc::new(FakePolicyEngine::new([response])));
            let error = use_case.execute(input()).await.unwrap_err();
            assert_eq!(error.code(), expected);
        }
    }
}
