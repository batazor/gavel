use crate::domain::{Description, LotId, PrincipalId, Title};

use super::{
    ApplicationError, Authorization, CatalogCommitError, CatalogEvent, CatalogUnitOfWork,
    CommandExecutor, CommandReceiptStore, IdGenerator, LotCommandResult, Permission,
    PermissionCheck, PortError, ReceiptIdentity, UpdateLotCommit, UpdateLotInput,
};

pub struct UpdateLot<'a, R, A, U, I> {
    executor: CommandExecutor<'a, R>,
    authorization: &'a A,
    unit_of_work: &'a U,
    ids: &'a I,
    dependency_timeout_millis: u64,
}

impl<'a, R, A, U, I> UpdateLot<'a, R, A, U, I>
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
        input: UpdateLotInput,
    ) -> Result<LotCommandResult, ApplicationError> {
        let principal = input
            .context
            .principal
            .as_ref()
            .ok_or(ApplicationError::Unauthenticated)?;
        let seller = PrincipalId::new(principal.principal_id.clone())
            .map_err(|_| invalid("invalid_principal", "principal is invalid"))?;
        let lot_id = LotId::new(input.lot_id.clone())
            .map_err(|_| invalid("invalid_lot_id", "lot id is invalid"))?;
        let expected_version = input
            .expected_version
            .ok_or_else(|| invalid("expected_version_required", "expected version is required"))?;
        let title = input
            .title
            .as_ref()
            .map(|value| Title::new(value.clone()))
            .transpose()
            .map_err(|_| invalid("invalid_title", "title is invalid"))?;
        let description = input
            .description
            .as_ref()
            .map(|value| Description::new(value.clone()))
            .transpose()
            .map_err(|_| invalid("invalid_description", "description is invalid"))?;
        if title.is_none() && description.is_none() {
            return Err(invalid("empty_update", "update contains no changes"));
        }
        let payload = UpdateFingerprint::from(&input);
        let caller_scope = input.context.caller_scope.clone();
        let idempotency_key = input.context.idempotency_key.clone();
        let consistency_token = input.context.consistency_token.clone();
        let correlation_id = input.context.correlation_id.clone();
        let occurred_at_millis = input.occurred_at_millis;
        let event_id = self.ids.event_id().map_err(map_dependency)?;
        let receipt = ReceiptIdentity {
            caller_scope: caller_scope.clone(),
            idempotency_key: idempotency_key.clone(),
        };
        let authorization = self.authorization;
        let unit_of_work = self.unit_of_work;
        let timeout = self.dependency_timeout_millis;
        self.executor
            .execute(
                "update_lot",
                &caller_scope,
                &idempotency_key,
                &payload,
                || async move {
                    let decision = authorization
                        .check(PermissionCheck {
                            principal: seller.clone(),
                            permission: Permission::EditLot,
                            resource_id: Some(lot_id.to_string()),
                            consistency_token,
                            timeout_millis: timeout,
                        })
                        .await
                        .map_err(map_authorization)?;
                    if !decision.allowed {
                        return Err(ApplicationError::Forbidden);
                    }
                    unit_of_work
                        .update_lot(UpdateLotCommit {
                            lot_id,
                            seller,
                            expected_version,
                            title,
                            description,
                            event: CatalogEvent::LotUpdated {
                                event_id,
                                correlation_id,
                                occurred_at_millis,
                            },
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
#[serde(rename_all = "camelCase")]
struct UpdateFingerprint<'a> {
    lot_id: &'a str,
    title: &'a Option<String>,
    description: &'a Option<Option<String>>,
    expected_version: Option<u64>,
}

impl<'a> From<&'a UpdateLotInput> for UpdateFingerprint<'a> {
    fn from(input: &'a UpdateLotInput) -> Self {
        Self {
            lot_id: &input.lot_id,
            title: &input.title,
            description: &input.description,
            expected_version: input.expected_version,
        }
    }
}

fn invalid(reason: &'static str, message: &'static str) -> ApplicationError {
    ApplicationError::InvalidArgument {
        reason,
        safe_message: message.to_owned(),
    }
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
        CatalogCommitError::Domain(crate::domain::DomainError::DescriptionFrozen) => {
            ApplicationError::Conflict {
                reason: "description_frozen",
                safe_message: "lot description is frozen".to_owned(),
            }
        }
        CatalogCommitError::Domain(crate::domain::DomainError::VersionConflict)
        | CatalogCommitError::Conflict => ApplicationError::Conflict {
            reason: "version_conflict",
            safe_message: "lot version conflicts with current state".to_owned(),
        },
        CatalogCommitError::Domain(_) => ApplicationError::Conflict {
            reason: "invalid_lot_transition",
            safe_message: "lot cannot be updated".to_owned(),
        },
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
            ListLotCommit, LotOwnerGrant, PermissionDecision, ReceiptCompletion,
        },
        domain::{BidStep, ClosingPolicy, Currency, Description, Lot, Money, Title},
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
        owner: PrincipalId,
        checks: Mutex<usize>,
        failure: Mutex<Option<PortError>>,
    }

    #[async_trait]
    impl Authorization for FakeAuthorization {
        async fn check(&self, request: PermissionCheck) -> Result<PermissionDecision, PortError> {
            *self.checks.lock().unwrap() += 1;
            if let Some(error) = *self.failure.lock().unwrap() {
                return Err(error);
            }
            assert_eq!(request.permission, Permission::EditLot);
            assert_eq!(request.resource_id.as_deref(), Some("lot-1"));
            Ok(PermissionDecision {
                allowed: request.principal == self.owner,
                checked_at_token: "revision".to_owned(),
            })
        }

        async fn check_service(
            &self,
            _request: super::super::ServicePermissionCheck,
        ) -> Result<PermissionDecision, PortError> {
            unreachable!("UpdateLot uses a user principal")
        }

        async fn touch_lot_owner(
            &self,
            _request: LotOwnerGrant,
        ) -> Result<AuthorizationRevision, PortError> {
            unreachable!("update never provisions grants")
        }
    }

    struct FakeUnitOfWork {
        lot: Mutex<Lot>,
        outbox: Mutex<usize>,
    }

    #[async_trait]
    impl CatalogUnitOfWork for FakeUnitOfWork {
        async fn create_lot(
            &self,
            _commit: ListLotCommit,
        ) -> Result<LotCommandResult, CatalogCommitError> {
            unreachable!("not used by UpdateLot tests")
        }

        async fn update_lot(
            &self,
            commit: UpdateLotCommit,
        ) -> Result<LotCommandResult, CatalogCommitError> {
            let mut lot = self.lot.lock().unwrap();
            if lot.seller() != &commit.seller || lot.lot_id() != &commit.lot_id {
                return Err(CatalogCommitError::Conflict);
            }
            let mut candidate = lot.clone();
            candidate
                .update(commit.expected_version, commit.title, commit.description)
                .map_err(CatalogCommitError::Domain)?;
            *lot = candidate;
            *self.outbox.lock().unwrap() += 1;
            Ok(LotCommandResult {
                lot_id: lot.lot_id().to_string(),
                version: lot.version(),
            })
        }

        async fn withdraw_lot(
            &self,
            _commit: super::super::WithdrawLotCommit,
        ) -> Result<super::super::LotMutationResult, CatalogCommitError> {
            unreachable!("not used by UpdateLot tests")
        }
    }

    struct FakeIds;
    impl IdGenerator for FakeIds {
        fn image_id(&self) -> Result<crate::domain::ImageId, PortError> {
            unreachable!("not used by UpdateLot")
        }

        fn object_key(&self, _image_id: &crate::domain::ImageId) -> Result<String, PortError> {
            unreachable!("not used by UpdateLot")
        }

        fn event_id(&self) -> Result<String, PortError> {
            Ok("event-update-1".to_owned())
        }
    }

    fn lot() -> Lot {
        Lot::list(
            LotId::new("lot-1").unwrap(),
            PrincipalId::new("seller-1").unwrap(),
            Title::new("Old title").unwrap(),
            Description::new(Some("Old description".to_owned())).unwrap(),
            Money::new(100, Currency::new("USD").unwrap()).unwrap(),
            BidStep::new(10).unwrap(),
            ClosingPolicy::new(2_000, None, None).unwrap(),
            "authz-revision",
            1_000,
        )
        .unwrap()
    }

    fn input(owner: &str, key: &str, expected_version: u64) -> UpdateLotInput {
        UpdateLotInput {
            context: CommandContext {
                caller_scope: owner.to_owned(),
                principal: Some(super::super::Principal {
                    principal_id: owner.to_owned(),
                }),
                idempotency_key: key.to_owned(),
                correlation_id: "correlation-update".to_owned(),
                consistency_token: None,
            },
            lot_id: "lot-1".to_owned(),
            title: Some("New title".to_owned()),
            description: None,
            expected_version: Some(expected_version),
            occurred_at_millis: 1_100,
        }
    }

    fn fake_auth() -> FakeAuthorization {
        FakeAuthorization {
            owner: PrincipalId::new("seller-1").unwrap(),
            checks: Mutex::new(0),
            failure: Mutex::new(None),
        }
    }

    #[tokio::test]
    async fn successful_update_writes_one_outbox_and_exact_replay_skips_effects() {
        let receipts = Receipts::default();
        let auth = fake_auth();
        let uow = FakeUnitOfWork {
            lot: Mutex::new(lot()),
            outbox: Mutex::new(0),
        };
        let service = UpdateLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        let result = service
            .execute(input("seller-1", "key-1", 1))
            .await
            .unwrap();
        assert_eq!(result.version, 2);
        assert_eq!(
            service
                .execute(input("seller-1", "key-1", 1))
                .await
                .unwrap(),
            result
        );
        assert_eq!(*uow.outbox.lock().unwrap(), 1);
        assert_eq!(*auth.checks.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn non_owner_and_authz_failure_never_change_lot() {
        for dependency_failure in [false, true] {
            let receipts = Receipts::default();
            let auth = fake_auth();
            if dependency_failure {
                *auth.failure.lock().unwrap() = Some(PortError::Timeout);
            }
            let uow = FakeUnitOfWork {
                lot: Mutex::new(lot()),
                outbox: Mutex::new(0),
            };
            let service = UpdateLot::new(
                &receipts,
                b"test-hmac-key-at-least-32-bytes!",
                &auth,
                &uow,
                &FakeIds,
                250,
            );
            let owner = if dependency_failure {
                "seller-1"
            } else {
                "seller-2"
            };
            assert!(service.execute(input(owner, "key-1", 1)).await.is_err());
            assert_eq!(uow.lot.lock().unwrap().version(), 1);
            assert_eq!(*uow.outbox.lock().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn frozen_and_concurrent_version_conflicts_do_not_write_extra_events() {
        let mut frozen = lot();
        frozen.freeze_description(1);
        let receipts = Receipts::default();
        let auth = fake_auth();
        let uow = FakeUnitOfWork {
            lot: Mutex::new(frozen),
            outbox: Mutex::new(0),
        };
        let service = UpdateLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        let error = service
            .execute(input("seller-1", "frozen", 1))
            .await
            .unwrap_err();
        assert_eq!(error.reason(), "description_frozen");
        assert_eq!(*uow.outbox.lock().unwrap(), 0);

        let receipts = Receipts::default();
        let auth = fake_auth();
        let uow = FakeUnitOfWork {
            lot: Mutex::new(lot()),
            outbox: Mutex::new(0),
        };
        let service = UpdateLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        let (first, second) = tokio::join!(
            service.execute(input("seller-1", "key-a", 1)),
            service.execute(input("seller-1", "key-b", 1))
        );
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        let rejection = if first.is_err() { first } else { second };
        assert_eq!(rejection.unwrap_err().reason(), "version_conflict");
        assert_eq!(uow.lot.lock().unwrap().version(), 2);
        assert_eq!(*uow.outbox.lock().unwrap(), 1);
    }
}
