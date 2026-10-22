use sha2::{Digest, Sha256};

use crate::domain::{
    AltText, BidStep, ClosingPolicy, Currency, Description, ImageAttachment, ImageId, Lot, LotId,
    Money, PrincipalId, Title,
};

use super::{
    ApplicationError, Authorization, CatalogCommitError, CatalogEvent, CatalogUnitOfWork,
    CommandExecutor, CommandReceiptStore, IdGenerator, ListLotCommit, ListLotInput,
    LotCommandResult, LotOwnerGrant, Permission, PermissionCheck, PortError, ReceiptIdentity,
};

pub struct ListLot<'a, R, A, U, I> {
    executor: CommandExecutor<'a, R>,
    authorization: &'a A,
    unit_of_work: &'a U,
    ids: &'a I,
    dependency_timeout_millis: u64,
}

impl<'a, R, A, U, I> ListLot<'a, R, A, U, I>
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

    pub async fn execute(&self, input: ListLotInput) -> Result<LotCommandResult, ApplicationError> {
        let principal = input
            .context
            .principal
            .as_ref()
            .ok_or(ApplicationError::Unauthenticated)?;
        let seller = PrincipalId::new(principal.principal_id.clone())
            .map_err(|_| invalid("invalid_principal", "principal is invalid"))?;
        let lot_id = LotId::new(input.lot_id.clone())
            .map_err(|_| invalid("invalid_lot_id", "lot id is invalid"))?;
        let title = Title::new(input.title.clone())
            .map_err(|_| invalid("invalid_title", "title is invalid"))?;
        let description = Description::new(input.description.clone())
            .map_err(|_| invalid("invalid_description", "description is invalid"))?;
        let currency = Currency::new(input.start_price.currency_code.clone())
            .map_err(|_| invalid("invalid_money", "money is invalid"))?;
        let start_price = Money::new(input.start_price.minor, currency)
            .map_err(|_| invalid("invalid_money", "money is invalid"))?;
        let bid_step = BidStep::new(input.bid_step_minor)
            .map_err(|_| invalid("invalid_bid_step", "bid step is invalid"))?;
        let closing_policy = ClosingPolicy::new(
            input.closing_policy.scheduled_end_millis,
            input.closing_policy.inactivity_millis,
            input.closing_policy.hard_deadline_millis,
        )
        .map_err(|_| invalid("invalid_closing_policy", "closing policy is invalid"))?;
        let attachments = input
            .images
            .iter()
            .map(|image| {
                Ok(ImageAttachment {
                    image_id: ImageId::new(image.image_id.clone())
                        .map_err(|_| invalid("invalid_image_id", "image attachment is invalid"))?,
                    alt: AltText::new(image.alt.clone())
                        .map_err(|_| invalid("invalid_image_alt", "image attachment is invalid"))?,
                })
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        if !(1..=20).contains(&attachments.len()) {
            return Err(invalid(
                "invalid_image_batch",
                "image attachment batch is invalid",
            ));
        }

        let payload = ListLotFingerprint::from(&input);
        let caller_scope = input.context.caller_scope.clone();
        let idempotency_key = input.context.idempotency_key.clone();
        let consistency_token = input.context.consistency_token.clone();
        let correlation_id = input.context.correlation_id.clone();
        let occurred_at_millis = input.occurred_at_millis;
        let event_id = self.ids.event_id().map_err(map_dependency)?;
        let grant_idempotency_key = grant_key(&caller_scope, &idempotency_key, &lot_id);
        let receipt = ReceiptIdentity {
            caller_scope: caller_scope.clone(),
            idempotency_key: idempotency_key.clone(),
        };
        let authorization = self.authorization;
        let unit_of_work = self.unit_of_work;
        let timeout = self.dependency_timeout_millis;
        self.executor
            .execute(
                "list_lot",
                &caller_scope,
                &idempotency_key,
                &payload,
                || async move {
                    let decision = authorization
                        .check(PermissionCheck {
                            principal: seller.clone(),
                            permission: Permission::CreateLot,
                            resource_id: None,
                            consistency_token: consistency_token.clone(),
                            timeout_millis: timeout,
                        })
                        .await
                        .map_err(map_authorization)?;
                    if !decision.allowed {
                        return Err(ApplicationError::Forbidden);
                    }
                    let revision = authorization
                        .touch_lot_owner(LotOwnerGrant {
                            lot_id: lot_id.to_string(),
                            seller: seller.clone(),
                            idempotency_key: grant_idempotency_key,
                            consistency_token: Some(decision.checked_at_token),
                            timeout_millis: timeout,
                        })
                        .await
                        .map_err(map_authorization)?;
                    let lot = Lot::list(
                        lot_id.clone(),
                        seller,
                        title,
                        description,
                        start_price,
                        bid_step,
                        closing_policy,
                        revision.revision,
                        occurred_at_millis,
                    )
                    .map_err(|_| invalid("invalid_lot", "lot is invalid"))?;
                    let result = LotCommandResult {
                        lot_id: lot_id.to_string(),
                        version: lot.version(),
                    };
                    unit_of_work
                        .create_lot(ListLotCommit {
                            lot,
                            attachments,
                            event: CatalogEvent::LotListed {
                                event_id,
                                correlation_id,
                                occurred_at_millis,
                            },
                            result,
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
struct ListLotFingerprint<'a> {
    lot_id: &'a str,
    title: &'a str,
    description: &'a Option<String>,
    images: &'a [super::ImageAttachmentInput],
    start_price: &'a super::MoneyInput,
    bid_step_minor: u64,
    closing_policy: &'a super::ClosingPolicyInput,
    occurred_at_millis: u64,
}

impl<'a> From<&'a ListLotInput> for ListLotFingerprint<'a> {
    fn from(input: &'a ListLotInput) -> Self {
        Self {
            lot_id: &input.lot_id,
            title: &input.title,
            description: &input.description,
            images: &input.images,
            start_price: &input.start_price,
            bid_step_minor: input.bid_step_minor,
            closing_policy: &input.closing_policy,
            occurred_at_millis: input.occurred_at_millis,
        }
    }
}

fn grant_key(caller_scope: &str, idempotency_key: &str, lot_id: &LotId) -> String {
    let mut digest = Sha256::new();
    digest.update(b"catalog-list-lot-grant\0");
    digest.update(caller_scope.as_bytes());
    digest.update([0]);
    digest.update(idempotency_key.as_bytes());
    digest.update([0]);
    digest.update(lot_id.as_str().as_bytes());
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        digest.finalize(),
    )
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
        CatalogCommitError::Domain(_) => ApplicationError::Conflict {
            reason: "invalid_image_batch",
            safe_message: "image attachment batch is invalid".to_owned(),
        },
        CatalogCommitError::Conflict => ApplicationError::Conflict {
            reason: "lot_conflict",
            safe_message: "lot conflicts with current state".to_owned(),
        },
        CatalogCommitError::Port(error) => match error {
            PortError::Timeout | PortError::Unavailable => {
                ApplicationError::DependencyUnavailable {
                    dependency: "catalog_database",
                }
            }
            PortError::NotFound | PortError::Internal => ApplicationError::Internal,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use base64::{Engine, engine::general_purpose::STANDARD};

    use crate::{
        application::{
            AuthorizationRevision, ClosingPolicyInput, CommandContext, CommandReceiptClaim,
            CommandReceiptOutcome, ImageAttachmentInput, MoneyInput, PermissionDecision,
            ReceiptCompletion,
        },
        domain::{ActualObjectMetadata, LotImage, ObjectMetadata, attach_images},
    };

    use super::*;

    #[derive(Clone)]
    struct Receipt {
        fingerprint: String,
        token: String,
        result: Option<Vec<u8>>,
    }

    #[derive(Default)]
    struct Receipts(Mutex<HashMap<(String, String, &'static str), Receipt>>);

    #[async_trait]
    impl CommandReceiptStore for Receipts {
        async fn claim(
            &self,
            claim: CommandReceiptClaim,
        ) -> Result<CommandReceiptOutcome, PortError> {
            let mut receipts = self.0.lock().unwrap();
            let key = (claim.caller_scope, claim.idempotency_key, claim.command);
            if let Some(receipt) = receipts.get(&key) {
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
            let token = format!("claim-{}", receipts.len() + 1);
            receipts.insert(
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
            let mut receipts = self.0.lock().unwrap();
            let Some(receipt) = receipts
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

    struct AuthState {
        allowed: bool,
        failure: Option<PortError>,
        checks: Vec<PermissionCheck>,
        grants: Vec<LotOwnerGrant>,
    }

    struct FakeAuthorization {
        state: Mutex<AuthState>,
        order: Arc<Mutex<Vec<&'static str>>>,
    }

    impl FakeAuthorization {
        fn new(order: Arc<Mutex<Vec<&'static str>>>) -> Self {
            Self {
                state: Mutex::new(AuthState {
                    allowed: true,
                    failure: None,
                    checks: Vec::new(),
                    grants: Vec::new(),
                }),
                order,
            }
        }
    }

    #[async_trait]
    impl Authorization for FakeAuthorization {
        async fn check(&self, request: PermissionCheck) -> Result<PermissionDecision, PortError> {
            self.order.lock().unwrap().push("check");
            let mut state = self.state.lock().unwrap();
            state.checks.push(request);
            if let Some(error) = state.failure {
                return Err(error);
            }
            Ok(PermissionDecision {
                allowed: state.allowed,
                checked_at_token: "check-revision".to_owned(),
            })
        }

        async fn check_service(
            &self,
            _request: super::super::ServicePermissionCheck,
        ) -> Result<PermissionDecision, PortError> {
            unreachable!("ListLot uses a user principal")
        }

        async fn touch_lot_owner(
            &self,
            request: LotOwnerGrant,
        ) -> Result<AuthorizationRevision, PortError> {
            self.order.lock().unwrap().push("touch");
            let mut state = self.state.lock().unwrap();
            state.grants.push(request);
            if let Some(error) = state.failure {
                return Err(error);
            }
            Ok(AuthorizationRevision {
                revision: "grant-revision".to_owned(),
            })
        }
    }

    #[derive(Default)]
    struct UowState {
        images: HashMap<ImageId, LotImage>,
        lots: HashMap<LotId, Lot>,
        results: HashMap<LotId, LotCommandResult>,
        outbox_count: usize,
        fail_after_commit_once: bool,
    }

    struct FakeUnitOfWork {
        state: Mutex<UowState>,
        order: Arc<Mutex<Vec<&'static str>>>,
    }

    impl FakeUnitOfWork {
        fn new(order: Arc<Mutex<Vec<&'static str>>>) -> Self {
            Self {
                state: Mutex::new(UowState::default()),
                order,
            }
        }

        fn insert_image(&self, image: LotImage) {
            self.state
                .lock()
                .unwrap()
                .images
                .insert(image.image_id().clone(), image);
        }
    }

    #[async_trait]
    impl CatalogUnitOfWork for FakeUnitOfWork {
        async fn create_lot(
            &self,
            commit: ListLotCommit,
        ) -> Result<LotCommandResult, CatalogCommitError> {
            self.order.lock().unwrap().push("commit");
            let mut state = self.state.lock().unwrap();
            if let Some(result) = state.results.get(commit.lot.lot_id()) {
                return Ok(result.clone());
            }
            let mut images = commit
                .attachments
                .iter()
                .filter_map(|attachment| state.images.get(&attachment.image_id).cloned())
                .collect::<Vec<_>>();
            attach_images(
                &mut images,
                &commit.attachments,
                commit.lot.seller(),
                commit.lot.lot_id(),
            )
            .map_err(CatalogCommitError::Domain)?;
            for image in images {
                state.images.insert(image.image_id().clone(), image);
            }
            state
                .lots
                .insert(commit.lot.lot_id().clone(), commit.lot.clone());
            state
                .results
                .insert(commit.lot.lot_id().clone(), commit.result.clone());
            state.outbox_count += 1;
            if state.fail_after_commit_once {
                state.fail_after_commit_once = false;
                return Err(CatalogCommitError::Port(PortError::Unavailable));
            }
            Ok(commit.result)
        }

        async fn update_lot(
            &self,
            _commit: super::super::UpdateLotCommit,
        ) -> Result<LotCommandResult, CatalogCommitError> {
            unreachable!("not used by ListLot tests")
        }

        async fn withdraw_lot(
            &self,
            _commit: super::super::WithdrawLotCommit,
        ) -> Result<super::super::LotMutationResult, CatalogCommitError> {
            unreachable!("not used by ListLot tests")
        }
    }

    struct FakeIds;
    impl IdGenerator for FakeIds {
        fn image_id(&self) -> Result<ImageId, PortError> {
            ImageId::new("generated-image").map_err(|_| PortError::Internal)
        }

        fn object_key(&self, image_id: &ImageId) -> Result<String, PortError> {
            Ok(format!("private/{image_id}"))
        }

        fn event_id(&self) -> Result<String, PortError> {
            Ok("event-1".to_owned())
        }
    }

    fn ready_image(id: &str, owner: &str) -> LotImage {
        let checksum = STANDARD.encode([9_u8; 32]);
        let mut image = LotImage::pending(
            ImageId::new(id).unwrap(),
            PrincipalId::new(owner).unwrap(),
            format!("private/{id}"),
            ObjectMetadata::new("image/png", 42, checksum.clone()).unwrap(),
            3_000,
        )
        .unwrap();
        image
            .verify(
                &PrincipalId::new(owner).unwrap(),
                &ActualObjectMetadata {
                    content_type: "image/png".to_owned(),
                    content_length: 42,
                    checksum_sha256: checksum,
                },
                1_000,
            )
            .unwrap();
        image
    }

    fn input(key: &str) -> ListLotInput {
        ListLotInput {
            context: CommandContext {
                caller_scope: "seller-1".to_owned(),
                principal: Some(super::super::Principal {
                    principal_id: "seller-1".to_owned(),
                }),
                idempotency_key: key.to_owned(),
                correlation_id: "correlation-1".to_owned(),
                consistency_token: Some("client-revision".to_owned()),
            },
            lot_id: "lot-1".to_owned(),
            title: "Vintage clock".to_owned(),
            description: Some("Works".to_owned()),
            images: vec![ImageAttachmentInput {
                image_id: "image-1".to_owned(),
                alt: "front".to_owned(),
            }],
            start_price: MoneyInput {
                minor: 100,
                currency_code: "USD".to_owned(),
            },
            bid_step_minor: 10,
            closing_policy: ClosingPolicyInput {
                scheduled_end_millis: 2_000,
                inactivity_millis: Some(100),
                hard_deadline_millis: Some(3_000),
            },
            occurred_at_millis: 1_000,
        }
    }

    #[tokio::test]
    async fn list_lot_checks_then_touches_then_commits_once_and_replays() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let receipts = Receipts::default();
        let auth = FakeAuthorization::new(order.clone());
        let uow = FakeUnitOfWork::new(order.clone());
        uow.insert_image(ready_image("image-1", "seller-1"));
        let service = ListLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        let result = service.execute(input("key-1")).await.unwrap();
        assert_eq!(result.lot_id, "lot-1");
        assert_eq!(*order.lock().unwrap(), ["check", "touch", "commit"]);
        {
            let state = uow.state.lock().unwrap();
            assert_eq!(state.outbox_count, 1);
            assert_eq!(
                state
                    .lots
                    .get(&LotId::new("lot-1").unwrap())
                    .unwrap()
                    .authz_revision(),
                "grant-revision"
            );
        }
        {
            let auth_state = auth.state.lock().unwrap();
            assert_eq!(auth_state.grants.len(), 1);
            let grant = &auth_state.grants[0];
            assert_eq!(grant.seller.as_str(), "seller-1");
            assert_eq!(grant.lot_id, "lot-1");
            assert_eq!(grant.consistency_token.as_deref(), Some("check-revision"));
            assert_eq!(grant.timeout_millis, 250);
        }

        assert_eq!(service.execute(input("key-1")).await.unwrap(), result);
        assert_eq!(uow.state.lock().unwrap().outbox_count, 1);
        assert_eq!(auth.state.lock().unwrap().grants.len(), 1);
    }

    #[tokio::test]
    async fn deny_and_authorization_failure_are_fail_closed_before_commit() {
        for failure in [None, Some(PortError::Timeout)] {
            let order = Arc::new(Mutex::new(Vec::new()));
            let receipts = Receipts::default();
            let auth = FakeAuthorization::new(order.clone());
            {
                let mut state = auth.state.lock().unwrap();
                state.allowed = false;
                state.failure = failure;
            }
            let uow = FakeUnitOfWork::new(order);
            uow.insert_image(ready_image("image-1", "seller-1"));
            let service = ListLot::new(
                &receipts,
                b"test-hmac-key-at-least-32-bytes!",
                &auth,
                &uow,
                &FakeIds,
                250,
            );
            let error = service.execute(input("key-1")).await.unwrap_err();
            if failure.is_some() {
                assert!(matches!(
                    error,
                    ApplicationError::DependencyUnavailable { .. }
                ));
            } else {
                assert_eq!(error, ApplicationError::Forbidden);
            }
            assert!(uow.state.lock().unwrap().lots.is_empty());
            assert_eq!(uow.state.lock().unwrap().outbox_count, 0);
        }
    }

    #[tokio::test]
    async fn ambiguity_window_retry_reuses_grant_and_does_not_duplicate_outbox() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let receipts = Receipts::default();
        let auth = FakeAuthorization::new(order.clone());
        let uow = FakeUnitOfWork::new(order);
        uow.insert_image(ready_image("image-1", "seller-1"));
        uow.state.lock().unwrap().fail_after_commit_once = true;
        let service = ListLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        assert!(matches!(
            service.execute(input("key-1")).await,
            Err(ApplicationError::DependencyUnavailable { .. })
        ));
        let result = service.execute(input("key-1")).await.unwrap();
        assert_eq!(result.version, 1);
        assert_eq!(uow.state.lock().unwrap().outbox_count, 1);
        let grants = &auth.state.lock().unwrap().grants;
        assert_eq!(grants.len(), 2);
        assert_eq!(grants[0].idempotency_key, grants[1].idempotency_key);
    }

    #[tokio::test]
    async fn invalid_or_foreign_image_batch_never_commits_a_lot_or_outbox() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let receipts = Receipts::default();
        let auth = FakeAuthorization::new(order.clone());
        let uow = FakeUnitOfWork::new(order);
        uow.insert_image(ready_image("image-1", "seller-2"));
        let service = ListLot::new(
            &receipts,
            b"test-hmac-key-at-least-32-bytes!",
            &auth,
            &uow,
            &FakeIds,
            250,
        );
        assert!(matches!(
            service.execute(input("key-1")).await,
            Err(ApplicationError::Conflict { .. })
        ));
        let state = uow.state.lock().unwrap();
        assert!(state.lots.is_empty());
        assert_eq!(state.outbox_count, 0);
    }
}
