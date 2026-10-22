use crate::domain::LotId;

use super::{
    ApplicationError, Authorization, CatalogCommitError, CatalogEvent, CatalogUnitOfWork,
    CommandExecutor, CommandReceiptStore, IdGenerator, LotMutationResult, Permission, PortError,
    ReceiptIdentity, ServicePermissionCheck, WithdrawLotCommit, WithdrawLotInput, WorkloadIdentity,
};

pub struct WithdrawLot<'a, R, A, U, I> {
    executor: CommandExecutor<'a, R>,
    authorization: &'a A,
    unit_of_work: &'a U,
    ids: &'a I,
    dependency_timeout_millis: u64,
}

impl<'a, R, A, U, I> WithdrawLot<'a, R, A, U, I>
where
    R: CommandReceiptStore,
    A: Authorization,
    U: CatalogUnitOfWork,
    I: IdGenerator,
{
    pub const fn new(
        receipts: &'a R,
        fingerprint_key: &'a [u8],
        authorization: &'a A,
        unit_of_work: &'a U,
        ids: &'a I,
        dependency_timeout_millis: u64,
    ) -> Self {
        Self {
            executor: CommandExecutor::new(receipts, fingerprint_key),
            authorization,
            unit_of_work,
            ids,
            dependency_timeout_millis,
        }
    }

    pub async fn execute(
        &self,
        input: WithdrawLotInput,
    ) -> Result<LotMutationResult, ApplicationError> {
        if input.caller != WorkloadIdentity::Processes {
            return Err(ApplicationError::Forbidden);
        }
        let lot_id =
            LotId::new(input.lot_id.clone()).map_err(|_| ApplicationError::InvalidArgument {
                reason: "invalid_lot_id",
                safe_message: "lot id is invalid".to_owned(),
            })?;
        let payload = WithdrawFingerprint {
            lot_id: &input.lot_id,
        };
        let caller_scope = input.context.caller_scope.clone();
        let idempotency_key = input.context.idempotency_key.clone();
        let consistency_token = input.context.consistency_token.clone();
        let correlation_id = input.context.correlation_id.clone();
        let event_id = self.ids.event_id().map_err(map_dependency)?;
        let receipt = ReceiptIdentity {
            caller_scope: caller_scope.clone(),
            idempotency_key: idempotency_key.clone(),
        };
        let occurred_at_millis = input.occurred_at_millis;
        let authorization = self.authorization;
        let unit_of_work = self.unit_of_work;
        let timeout = self.dependency_timeout_millis;
        self.executor
            .execute(
                "withdraw_lot",
                &caller_scope,
                &idempotency_key,
                &payload,
                || async move {
                    let decision = authorization
                        .check_service(ServicePermissionCheck {
                            workload: WorkloadIdentity::Processes,
                            permission: Permission::ExecuteWithdraw,
                            resource_id: lot_id.to_string(),
                            consistency_token,
                            timeout_millis: timeout,
                        })
                        .await
                        .map_err(map_authorization)?;
                    if !decision.allowed {
                        return Err(ApplicationError::Forbidden);
                    }
                    unit_of_work
                        .withdraw_lot(WithdrawLotCommit {
                            lot_id,
                            event: CatalogEvent::LotWithdrawn {
                                event_id,
                                correlation_id,
                                occurred_at_millis,
                            },
                            checked_at_token: decision.checked_at_token,
                            receipt,
                        })
                        .await
                        .map_err(map_commit)
                },
            )
            .await
    }
}

#[derive(serde::Serialize)]
struct WithdrawFingerprint<'a> {
    lot_id: &'a str,
}

fn map_authorization(error: PortError) -> ApplicationError {
    match error {
        PortError::Timeout | PortError::Unavailable => ApplicationError::DependencyUnavailable {
            dependency: "authorization",
        },
        PortError::NotFound | PortError::Internal => ApplicationError::Internal,
    }
}

fn map_dependency(error: PortError) -> ApplicationError {
    match error {
        PortError::Timeout | PortError::Unavailable => ApplicationError::DependencyUnavailable {
            dependency: "id_generator",
        },
        PortError::NotFound | PortError::Internal => ApplicationError::Internal,
    }
}

fn map_commit(error: CatalogCommitError) -> ApplicationError {
    match error {
        CatalogCommitError::Domain(_) | CatalogCommitError::Conflict => {
            ApplicationError::Conflict {
                reason: "withdraw_conflict",
                safe_message: "lot cannot be withdrawn".to_owned(),
            }
        }
        CatalogCommitError::Port(PortError::Timeout | PortError::Unavailable) => {
            ApplicationError::DependencyUnavailable {
                dependency: "catalog_database",
            }
        }
        CatalogCommitError::Port(PortError::NotFound) => ApplicationError::NotFound,
        CatalogCommitError::Port(PortError::Internal) => ApplicationError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;

    use crate::{
        application::{
            AuthorizationRevision, CommandContext, CommandReceiptClaim, CommandReceiptOutcome,
            ListLotCommit, LotCommandResult, LotOwnerGrant, PermissionCheck, PermissionDecision,
            ReceiptCompletion, UpdateLotCommit,
        },
        domain::{
            BidStep, ClosingPolicy, Currency, Description, Lot, LotState, Money, PrincipalId, Title,
        },
    };

    use super::*;

    #[derive(Clone)]
    struct Receipt {
        fingerprint: String,
        token: String,
        result: Option<Vec<u8>>,
    }

    #[derive(Default)]
    struct Receipts(Mutex<HashMap<(String, String), Receipt>>);

    #[async_trait]
    impl CommandReceiptStore for Receipts {
        async fn claim(
            &self,
            claim: CommandReceiptClaim,
        ) -> Result<CommandReceiptOutcome, PortError> {
            let mut state = self.0.lock().unwrap();
            let key = (claim.caller_scope, claim.idempotency_key);
            if let Some(receipt) = state.get(&key) {
                if receipt.fingerprint != claim.fingerprint {
                    return Ok(CommandReceiptOutcome::Conflict);
                }
                return Ok(receipt.result.as_ref().map_or(
                    CommandReceiptOutcome::InProgress,
                    |result| CommandReceiptOutcome::Replay {
                        result_json: result.clone(),
                    },
                ));
            }
            let token = format!("claim-{}", state.len() + 1);
            state.insert(
                key,
                Receipt {
                    fingerprint: claim.fingerprint,
                    token: token.clone(),
                    result: None,
                },
            );
            Ok(CommandReceiptOutcome::Acquired { claim_token: token })
        }

        async fn complete(
            &self,
            claim_token: &str,
            result_json: &[u8],
        ) -> Result<ReceiptCompletion, PortError> {
            let mut state = self.0.lock().unwrap();
            let Some(receipt) = state
                .values_mut()
                .find(|receipt| receipt.token == claim_token)
            else {
                return Ok(ReceiptCompletion::LostClaim);
            };
            receipt.result = Some(result_json.to_vec());
            Ok(ReceiptCompletion::Stored)
        }

        async fn release_retry(&self, claim_token: &str) -> Result<(), PortError> {
            self.0
                .lock()
                .unwrap()
                .retain(|_, receipt| receipt.token != claim_token);
            Ok(())
        }
    }

    struct FakeAuthorization {
        allowed: Mutex<bool>,
        failure: Mutex<Option<PortError>>,
        calls: Mutex<Vec<ServicePermissionCheck>>,
    }

    #[async_trait]
    impl Authorization for FakeAuthorization {
        async fn check(&self, _request: PermissionCheck) -> Result<PermissionDecision, PortError> {
            unreachable!("WithdrawLot uses a workload identity")
        }

        async fn check_service(
            &self,
            request: ServicePermissionCheck,
        ) -> Result<PermissionDecision, PortError> {
            self.calls.lock().unwrap().push(request.clone());
            if let Some(error) = *self.failure.lock().unwrap() {
                return Err(error);
            }
            assert_eq!(request.workload, WorkloadIdentity::Processes);
            assert_eq!(request.permission, Permission::ExecuteWithdraw);
            Ok(PermissionDecision {
                allowed: *self.allowed.lock().unwrap(),
                checked_at_token: "checked-revision".to_owned(),
            })
        }

        async fn touch_lot_owner(
            &self,
            _request: LotOwnerGrant,
        ) -> Result<AuthorizationRevision, PortError> {
            unreachable!("WithdrawLot never provisions grants")
        }
    }

    struct FakeUnitOfWork {
        lots: Mutex<HashMap<LotId, Lot>>,
        outbox: Mutex<usize>,
    }

    #[async_trait]
    impl CatalogUnitOfWork for FakeUnitOfWork {
        async fn create_lot(
            &self,
            _commit: ListLotCommit,
        ) -> Result<LotCommandResult, CatalogCommitError> {
            unreachable!("not used by WithdrawLot tests")
        }

        async fn update_lot(
            &self,
            _commit: UpdateLotCommit,
        ) -> Result<LotCommandResult, CatalogCommitError> {
            unreachable!("not used by WithdrawLot tests")
        }

        async fn withdraw_lot(
            &self,
            commit: WithdrawLotCommit,
        ) -> Result<LotMutationResult, CatalogCommitError> {
            let mut lots = self.lots.lock().unwrap();
            let lot = lots
                .get_mut(&commit.lot_id)
                .ok_or(CatalogCommitError::Port(PortError::NotFound))?;
            let was_listed = lot.state() == LotState::Listed;
            let current = lot.version();
            lot.withdraw(current).map_err(CatalogCommitError::Domain)?;
            if was_listed {
                *self.outbox.lock().unwrap() += 1;
            }
            Ok(LotMutationResult {
                lot_id: lot.lot_id().to_string(),
                version: lot.version(),
                checked_at_token: commit.checked_at_token,
            })
        }
    }

    struct FakeIds;
    impl IdGenerator for FakeIds {
        fn image_id(&self) -> Result<crate::domain::ImageId, PortError> {
            unreachable!("not used by WithdrawLot")
        }

        fn object_key(&self, _image_id: &crate::domain::ImageId) -> Result<String, PortError> {
            unreachable!("not used by WithdrawLot")
        }

        fn event_id(&self) -> Result<String, PortError> {
            Ok("event-withdraw-1".to_owned())
        }
    }

    fn lot(id: &str) -> Lot {
        Lot::list(
            LotId::new(id).unwrap(),
            PrincipalId::new("seller-1").unwrap(),
            Title::new("Lot").unwrap(),
            Description::new(None).unwrap(),
            Money::new(100, Currency::new("USD").unwrap()).unwrap(),
            BidStep::new(10).unwrap(),
            ClosingPolicy::new(2_000, None, None).unwrap(),
            "authz-revision",
            1_000,
        )
        .unwrap()
    }

    fn input(key: &str, lot_id: &str) -> WithdrawLotInput {
        WithdrawLotInput {
            context: CommandContext {
                caller_scope: "service-account:processes".to_owned(),
                principal: None,
                idempotency_key: key.to_owned(),
                correlation_id: "correlation-withdraw".to_owned(),
                consistency_token: Some("causal-token".to_owned()),
            },
            caller: WorkloadIdentity::Processes,
            lot_id: lot_id.to_owned(),
            occurred_at_millis: 1_500,
        }
    }

    fn auth() -> FakeAuthorization {
        FakeAuthorization {
            allowed: Mutex::new(true),
            failure: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn uow() -> FakeUnitOfWork {
        FakeUnitOfWork {
            lots: Mutex::new(HashMap::from([(
                LotId::new("lot-1").unwrap(),
                lot("lot-1"),
            )])),
            outbox: Mutex::new(0),
        }
    }

    #[tokio::test]
    async fn listed_withdraws_once_and_exact_or_already_withdrawn_replay_has_no_second_event() {
        let receipts = Receipts::default();
        let auth = auth();
        let uow = uow();
        let service = WithdrawLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        let result = service.execute(input("key-1", "lot-1")).await.unwrap();
        assert_eq!(result.version, 2);
        assert_eq!(result.checked_at_token, "checked-revision");
        assert_eq!(
            service.execute(input("key-1", "lot-1")).await.unwrap(),
            result
        );
        let already = service.execute(input("key-2", "lot-1")).await.unwrap();
        assert_eq!(already.version, 2);
        assert_eq!(*uow.outbox.lock().unwrap(), 1);
        assert_eq!(auth.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn caller_deny_and_authz_unavailable_fail_closed() {
        let receipts = Receipts::default();
        let auth = auth();
        let uow = uow();
        let service = WithdrawLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        let mut untrusted = input("untrusted", "lot-1");
        untrusted.caller = WorkloadIdentity::Untrusted;
        assert_eq!(
            service.execute(untrusted).await.unwrap_err(),
            ApplicationError::Forbidden
        );
        assert!(auth.calls.lock().unwrap().is_empty());

        *auth.allowed.lock().unwrap() = false;
        assert_eq!(
            service.execute(input("denied", "lot-1")).await.unwrap_err(),
            ApplicationError::Forbidden
        );
        *auth.allowed.lock().unwrap() = true;
        *auth.failure.lock().unwrap() = Some(PortError::Timeout);
        assert!(matches!(
            service.execute(input("failure", "lot-1")).await,
            Err(ApplicationError::DependencyUnavailable { .. })
        ));
        assert_eq!(
            uow.lots
                .lock()
                .unwrap()
                .get(&LotId::new("lot-1").unwrap())
                .unwrap()
                .state(),
            LotState::Listed
        );
        assert_eq!(*uow.outbox.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn changed_payload_for_same_key_conflicts_without_second_effect() {
        let receipts = Receipts::default();
        let auth = auth();
        let uow = uow();
        uow.lots
            .lock()
            .unwrap()
            .insert(LotId::new("lot-2").unwrap(), lot("lot-2"));
        let service = WithdrawLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        service.execute(input("key-1", "lot-1")).await.unwrap();
        assert_eq!(
            service.execute(input("key-1", "lot-2")).await.unwrap_err(),
            ApplicationError::IdempotencyConflict
        );
        assert_eq!(
            uow.lots
                .lock()
                .unwrap()
                .get(&LotId::new("lot-2").unwrap())
                .unwrap()
                .state(),
            LotState::Listed
        );
        assert_eq!(*uow.outbox.lock().unwrap(), 1);
    }
}
