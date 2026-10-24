use async_trait::async_trait;
use gavel_payments::{
    adapters::inbound::{
        AuthenticatedPrincipal, AuthenticatedWorkload, GrpcPayments, build_schema,
    },
    application::*,
    domain::*,
    rpc::{
        self, attempt_payment_response, expire_payment_response, get_settlement_attempt_response,
        payments_service_server::PaymentsService,
    },
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Default)]
struct MemoryState {
    instruments: HashMap<String, PaymentInstrument>,
    attempts: HashMap<String, PaymentAttempt>,
    events: Vec<DomainEvent>,
}
#[derive(Default)]
struct MemoryUow(Mutex<MemoryState>, AtomicU64);
#[async_trait]
impl PaymentsUnitOfWork for MemoryUow {
    async fn instrument_for_owner(
        &self,
        owner: &PrincipalId,
    ) -> Result<Option<PaymentInstrument>, PortError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .instruments
            .values()
            .find(|value| value.owner_id() == owner)
            .cloned())
    }
    async fn instrument(
        &self,
        id: &PaymentInstrumentId,
    ) -> Result<Option<PaymentInstrument>, PortError> {
        Ok(self.0.lock().unwrap().instruments.get(id.as_str()).cloned())
    }
    async fn instrument_by_session_ref(
        &self,
        value: &ProviderReference,
    ) -> Result<Option<PaymentInstrument>, PortError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .instruments
            .values()
            .find(|item| item.provider_session_ref() == value)
            .cloned())
    }
    async fn save_instrument(
        &self,
        commit: Commit<PaymentInstrument>,
        expected: Option<u64>,
    ) -> Result<(), CommitConflict> {
        if self
            .1
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_sub(1)
            })
            .is_ok()
        {
            return Err(CommitConflict::Version);
        }
        let mut state = self.0.lock().unwrap();
        if let Some(version) = expected {
            if state
                .instruments
                .get(commit.value.id().as_str())
                .map(PaymentInstrument::version)
                != Some(version)
            {
                return Err(CommitConflict::Version);
            }
        }
        state.events.extend(commit.events);
        state
            .instruments
            .insert(commit.value.id().to_string(), commit.value);
        Ok(())
    }
    async fn attempt(&self, id: &PaymentAttemptId) -> Result<Option<PaymentAttempt>, PortError> {
        Ok(self.0.lock().unwrap().attempts.get(id.as_str()).cloned())
    }
    async fn attempt_for_round(
        &self,
        auction: &str,
        round: u32,
    ) -> Result<Option<PaymentAttempt>, PortError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .attempts
            .values()
            .find(|value| {
                value.auction_id().as_str() == auction && value.settlement_round().get() == round
            })
            .cloned())
    }
    async fn attempt_by_operation_ref(
        &self,
        value: &ProviderReference,
    ) -> Result<Option<PaymentAttempt>, PortError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .attempts
            .values()
            .find(|item| item.provider_operation_ref() == Some(value))
            .cloned())
    }
    async fn save_attempt(
        &self,
        commit: Commit<PaymentAttempt>,
        expected: Option<u64>,
    ) -> Result<(), CommitConflict> {
        let mut state = self.0.lock().unwrap();
        if let Some(version) = expected {
            if state
                .attempts
                .get(commit.value.id().as_str())
                .map(PaymentAttempt::version)
                != Some(version)
            {
                return Err(CommitConflict::Version);
            }
        }
        if expected.is_none()
            && state.attempts.values().any(|item| {
                item.auction_id() == commit.value.auction_id()
                    && item.settlement_round() == commit.value.settlement_round()
            })
        {
            return Err(CommitConflict::AttemptForRound);
        }
        if commit.value.state() == PaymentAttemptState::Succeeded
            && state.attempts.values().any(|item| {
                item.auction_id() == commit.value.auction_id()
                    && item.state() == PaymentAttemptState::Succeeded
                    && item.id() != commit.value.id()
            })
        {
            return Err(CommitConflict::SuccessForAuction);
        }
        state.events.extend(commit.events);
        state
            .attempts
            .insert(commit.value.id().to_string(), commit.value);
        Ok(())
    }
    async fn stale_attempts(
        &self,
        _before: u64,
        limit: usize,
    ) -> Result<Vec<PaymentAttempt>, PortError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .attempts
            .values()
            .filter(|item| {
                matches!(
                    item.state(),
                    PaymentAttemptState::Processing | PaymentAttemptState::RecoveryRequired
                )
            })
            .take(limit)
            .cloned()
            .collect())
    }
    async fn revoked_instruments(&self, limit: usize) -> Result<Vec<PaymentInstrument>, PortError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .instruments
            .values()
            .filter(|item| item.state() == PaymentInstrumentState::Revoked)
            .take(limit)
            .cloned()
            .collect())
    }
}

struct FakeProvider {
    outcomes: Mutex<VecDeque<Result<ProviderOutcome, PortError>>>,
    calls: AtomicU64,
    prepare_keys: Mutex<Vec<String>>,
}
impl FakeProvider {
    fn new(outcomes: Vec<Result<ProviderOutcome, PortError>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            calls: AtomicU64::new(0),
            prepare_keys: Mutex::new(Vec::new()),
        }
    }
    fn next(&self) -> Result<ProviderOutcome, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(PortError::Internal))
    }
}
#[async_trait]
impl ProviderGateway for FakeProvider {
    async fn prepare(
        &self,
        request: PrepareInstrumentRequest,
    ) -> Result<PreparedInstrument, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.prepare_keys
            .lock()
            .unwrap()
            .push(request.provider_key.clone());
        Ok(PreparedInstrument {
            provider_url: "https://provider.example/register".to_owned(),
            provider_session_ref: ProviderReference::parse(format!(
                "session-{}",
                request.provider_key
            ))
            .unwrap(),
        })
    }
    async fn charge(&self, _: ChargeRequest) -> Result<ProviderOutcome, PortError> {
        self.next()
    }
    async fn continue_payment(
        &self,
        _: &ProviderReference,
        _: u64,
    ) -> Result<ProviderOutcome, PortError> {
        self.next()
    }
    async fn query_or_cancel(
        &self,
        _: &ProviderReference,
        _: u64,
    ) -> Result<ProviderOutcome, PortError> {
        self.next()
    }
}

struct FakeAuthz(AtomicBool);
#[async_trait]
impl Authorization for FakeAuthz {
    async fn check(&self, _: AuthorizationRequest) -> Result<AuthorizationDecision, PortError> {
        Ok(AuthorizationDecision {
            allowed: self.0.load(Ordering::SeqCst),
            checked_at_token: "token-1".to_owned(),
        })
    }
    async fn touch_owner_grant(
        &self,
        _: &str,
        _: &PrincipalId,
        _: &str,
        _: u64,
    ) -> Result<String, PortError> {
        Ok("token-2".to_owned())
    }
    async fn delete_owner_grant(
        &self,
        _: &str,
        _: &PrincipalId,
        _: &str,
        _: u64,
    ) -> Result<(), PortError> {
        Ok(())
    }
}
struct FixedClock;
impl Clock for FixedClock {
    fn now_millis(&self) -> u64 {
        1_000
    }
}
struct FakeIds(AtomicU64);
impl IdGenerator for FakeIds {
    fn instrument_id(&self) -> Result<PaymentInstrumentId, PortError> {
        Ok(PaymentInstrumentId::parse(format!(
            "instrument-{}",
            self.0.fetch_add(1, Ordering::SeqCst)
        ))
        .unwrap())
    }
    fn attempt_id(&self) -> Result<PaymentAttemptId, PortError> {
        Ok(
            PaymentAttemptId::parse(format!("attempt-{}", self.0.fetch_add(1, Ordering::SeqCst)))
                .unwrap(),
        )
    }
    fn event_id(&self) -> Result<String, PortError> {
        Ok(format!("event-{}", self.0.fetch_add(1, Ordering::SeqCst)))
    }
}
struct NoopTelemetry;
impl Telemetry for NoopTelemetry {
    fn command(&self, _: &'static str, _: &'static str) {}
}

type ReceiptKey = (String, &'static str, String);
type ReceiptRow = (String, String, Option<Vec<u8>>);
#[derive(Default)]
struct MemoryReceipts(Mutex<HashMap<ReceiptKey, ReceiptRow>>);
#[async_trait]
impl ReceiptStore for MemoryReceipts {
    async fn claim(&self, claim: ReceiptClaim) -> Result<ReceiptClaimResult, PortError> {
        let mut rows = self.0.lock().unwrap();
        let key = (claim.scope, claim.command, claim.key_digest);
        if let Some((fingerprint, _token, result)) = rows.get(&key) {
            return Ok(if fingerprint != &claim.fingerprint {
                ReceiptClaimResult::Conflict
            } else if let Some(value) = result {
                ReceiptClaimResult::Replay(value.clone())
            } else {
                ReceiptClaimResult::InProgress
            });
        }
        let token = format!("token-{}", rows.len());
        rows.insert(key, (claim.fingerprint, token.clone(), None));
        Ok(ReceiptClaimResult::Acquired { token })
    }
    async fn complete(&self, token: &str, result: &[u8]) -> Result<(), PortError> {
        let mut rows = self.0.lock().unwrap();
        let row = rows
            .values_mut()
            .find(|(_, value, _)| value == token)
            .ok_or(PortError::NotFound)?;
        row.2 = Some(result.to_vec());
        Ok(())
    }
    async fn release(&self, token: &str) -> Result<(), PortError> {
        self.0.lock().unwrap().retain(|_, row| row.1 != token);
        Ok(())
    }
}
type CallbackKey = (String, String);
type CallbackRow = (String, bool, u64);
#[derive(Default)]
struct MemoryCallbacks(Mutex<HashMap<CallbackKey, CallbackRow>>, AtomicU64);
#[async_trait]
impl CallbackInbox for MemoryCallbacks {
    async fn claim(
        &self,
        provider: &str,
        event: &str,
        fingerprint: &str,
        lease_until_millis: u64,
    ) -> Result<CallbackClaim, PortError> {
        let mut rows = self.0.lock().unwrap();
        let key = (provider.to_owned(), event.to_owned());
        Ok(match rows.get_mut(&key) {
            Some((value, _, _)) if value != fingerprint => CallbackClaim::Conflict,
            Some((_, true, _)) => CallbackClaim::Replay,
            Some((_, false, lease)) if *lease > 1_000 => CallbackClaim::InProgress,
            Some(value) => {
                value.2 = lease_until_millis;
                CallbackClaim::Acquired
            }
            None => {
                rows.insert(key, (fingerprint.to_owned(), false, lease_until_millis));
                CallbackClaim::Acquired
            }
        })
    }
    async fn complete(&self, provider: &str, event: &str) -> Result<(), PortError> {
        if self
            .1
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_sub(1)
            })
            .is_ok()
        {
            return Err(PortError::Unavailable);
        }
        if let Some(value) = self
            .0
            .lock()
            .unwrap()
            .get_mut(&(provider.to_owned(), event.to_owned()))
        {
            value.1 = true;
        }
        Ok(())
    }
}

fn app(
    allow: bool,
    outcomes: Vec<Result<ProviderOutcome, PortError>>,
) -> (
    Arc<PaymentApplication>,
    Arc<MemoryUow>,
    Arc<FakeProvider>,
    Arc<MemoryCallbacks>,
) {
    let uow = Arc::new(MemoryUow::default());
    let provider = Arc::new(FakeProvider::new(outcomes));
    let clock = Arc::new(FixedClock);
    let receipts = Arc::new(ReceiptExecutor::new(
        Arc::new(MemoryReceipts::default()),
        clock.clone(),
        CommandFingerprint::new([9_u8; 32]).unwrap(),
        30_000,
    ));
    let callbacks = Arc::new(MemoryCallbacks::default());
    (
        Arc::new(PaymentApplication {
            uow: uow.clone(),
            provider: provider.clone(),
            authz: Arc::new(FakeAuthz(AtomicBool::new(allow))),
            clock,
            ids: Arc::new(FakeIds(AtomicU64::new(1))),
            receipts,
            telemetry: Arc::new(NoopTelemetry),
            callbacks: callbacks.clone(),
        }),
        uow,
        provider,
        callbacks,
    )
}

fn registration() -> BeginRegistrationCommand {
    BeginRegistrationCommand {
        principal_id: "user-1".to_owned(),
        recovery_email: "owner@example.test".to_owned(),
        terms_version: "2026-10".to_owned(),
        idempotency_key: "register-1".to_owned(),
    }
}

#[tokio::test]
async fn registration_is_authorized_atomic_and_replayed_without_second_provider_effect() {
    let (app, _, provider, _) = app(true, vec![]);
    let first = app.begin_registration(registration()).await.unwrap();
    let replay = app.begin_registration(registration()).await.unwrap();
    assert_eq!(first, replay);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        app.my_instrument("user-1").await.unwrap().status,
        PaymentInstrumentState::Pending
    );
}

#[tokio::test]
async fn authorization_denial_is_fail_closed_before_provider_effect() {
    let (app, _, provider, _) = app(false, vec![]);
    assert_eq!(
        app.begin_registration(registration()).await.unwrap_err(),
        ApplicationError::PermissionDenied
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn registration_retries_local_commit_loss_with_same_opaque_provider_key() {
    let (app, uow, provider, _) = app(true, vec![]);
    uow.1.store(1, Ordering::SeqCst);
    assert_eq!(
        app.begin_registration(registration()).await.unwrap_err(),
        ApplicationError::InvalidState
    );
    let result = app.begin_registration(registration()).await.unwrap();
    assert_eq!(result.instrument.status, PaymentInstrumentState::Pending);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    let keys = provider.prepare_keys.lock().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], keys[1]);
    assert!(keys[0].starts_with("instrument:"));
    assert!(!keys[0].contains("user-1"));
    assert!(!keys[0].contains("register-1"));
}

#[tokio::test]
async fn attempt_continue_and_expire_reuse_one_provider_operation_and_hide_foreign_attempts() {
    let outcomes = vec![
        Ok(ProviderOutcome::RecoveryRequired {
            operation_ref: ProviderReference::parse("operation-1").unwrap(),
            recovery_url: "https://provider.example/recover".to_owned(),
        }),
        Ok(ProviderOutcome::Succeeded {
            operation_ref: ProviderReference::parse("operation-1").unwrap(),
        }),
    ];
    let (app, uow, provider, _) = app(true, outcomes);
    let mut instrument = PaymentInstrument::pending(
        PaymentInstrumentId::parse("instrument-ready").unwrap(),
        PrincipalId::parse("user-1").unwrap(),
        EmailAddress::parse("owner@example.test").unwrap(),
        DebitAuthorization::new(TermsVersion::parse("2026-10").unwrap(), 1),
        ProviderReference::parse("session-1").unwrap(),
        1,
    );
    instrument
        .mark_ready(
            ProviderReference::parse("customer-1").unwrap(),
            ProviderReference::parse("method-1").unwrap(),
            RedactedDisplay::parse("card").unwrap(),
            2,
        )
        .unwrap();
    uow.save_instrument(
        Commit {
            value: instrument,
            events: vec![],
        },
        None,
    )
    .await
    .unwrap();
    let command = AttemptPaymentCommand {
        workload_id: "spiffe-processes".to_owned(),
        auction_id: "auction-1".to_owned(),
        settlement_round: 1,
        candidate_id: "user-1".to_owned(),
        amount_minor_units: 100,
        currency_code: "USD".to_owned(),
        idempotency_key: "attempt-1".to_owned(),
        consistency_token: None,
    };
    let first = app.attempt_payment(command.clone()).await.unwrap();
    assert_eq!(first.status, PaymentAttemptState::RecoveryRequired);
    assert_eq!(app.attempt_payment(command).await.unwrap(), first);
    assert_eq!(
        app.payment_attempt("user-2", &first.id).await.unwrap_err(),
        ApplicationError::NotFound
    );
    let completed = app
        .continue_payment(ContinuePaymentCommand {
            principal_id: "user-1".to_owned(),
            attempt_id: first.id,
            idempotency_key: "continue-1".to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(completed.status, PaymentAttemptState::Succeeded);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn graphql_requires_context_principal_and_supports_registration_replay() {
    let (app, _, provider, _) = app(true, vec![]);
    let schema = build_schema(app);
    let query = r#"mutation { beginPaymentInstrumentRegistration(input:{recoveryEmail:"owner@example.test",termsVersion:"2026-10"},idempotencyKey:"register-1"){ instrument { id status version } providerUrl } }"#;
    assert!(!schema.execute(query).await.errors.is_empty());
    let request =
        async_graphql::Request::new(query).data(AuthenticatedPrincipal("user-1".to_owned()));
    assert!(schema.execute(request).await.errors.is_empty());
    let replay =
        async_graphql::Request::new(query).data(AuthenticatedPrincipal("user-1".to_owned()));
    assert!(schema.execute(replay).await.errors.is_empty());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn grpc_requires_trusted_extension_and_bounded_deadline_then_maps_typed_outcomes() {
    let outcomes = vec![
        Ok(ProviderOutcome::Processing {
            operation_ref: ProviderReference::parse("operation-grpc").unwrap(),
        }),
        Ok(ProviderOutcome::Declined),
    ];
    let (app, uow, _, _) = app(true, outcomes);
    let mut instrument = PaymentInstrument::pending(
        PaymentInstrumentId::parse("instrument-grpc").unwrap(),
        PrincipalId::parse("user-1").unwrap(),
        EmailAddress::parse("owner@example.test").unwrap(),
        DebitAuthorization::new(TermsVersion::parse("2026-10").unwrap(), 1),
        ProviderReference::parse("session-grpc").unwrap(),
        1,
    );
    instrument
        .mark_ready(
            ProviderReference::parse("customer-grpc").unwrap(),
            ProviderReference::parse("method-grpc").unwrap(),
            RedactedDisplay::parse("card").unwrap(),
            2,
        )
        .unwrap();
    uow.save_instrument(
        Commit {
            value: instrument,
            events: vec![],
        },
        None,
    )
    .await
    .unwrap();
    let service = GrpcPayments::new(
        app,
        "spiffe://cluster.local/ns/gavel/sa/gavel-processes".to_owned(),
        std::time::Duration::from_secs(15),
    );
    let message = rpc::AttemptPaymentRequest {
        auction_id: "auction-grpc".to_owned(),
        settlement_round: 1,
        candidate_id: "user-1".to_owned(),
        amount: Some(rpc::Money {
            minor_units: 100,
            currency_code: "USD".to_owned(),
        }),
        idempotency_key: "grpc-attempt-1".to_owned(),
        correlation_id: "correlation-1".to_owned(),
        consistency_token: String::new(),
    };
    let mut spoofed = tonic_bsr::Request::new(message.clone());
    spoofed.metadata_mut().insert(
        "x-spiffe-id",
        "spiffe://cluster.local/ns/gavel/sa/gavel-processes"
            .parse()
            .unwrap(),
    );
    spoofed
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    assert_eq!(
        service.attempt_payment(spoofed).await.unwrap_err().code(),
        tonic_bsr::Code::Unauthenticated
    );

    let mut request = tonic_bsr::Request::new(message);
    request
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    request.extensions_mut().insert(AuthenticatedWorkload {
        spiffe_id: "spiffe://cluster.local/ns/gavel/sa/gavel-processes".to_owned(),
    });
    let response = service.attempt_payment(request).await.unwrap().into_inner();
    let attempt_id = match response.outcome.unwrap() {
        attempt_payment_response::Outcome::Result(value) => {
            assert_eq!(value.status, rpc::PaymentAttemptStatus::Processing as i32);
            value.payment_attempt_id
        }
        other => panic!("unexpected {other:?}"),
    };

    let mut expire = tonic_bsr::Request::new(rpc::ExpirePaymentRequest {
        payment_attempt_id: attempt_id,
        idempotency_key: "expire-1".to_owned(),
        correlation_id: "correlation-2".to_owned(),
        consistency_token: String::new(),
    });
    expire
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    expire.extensions_mut().insert(AuthenticatedWorkload {
        spiffe_id: "spiffe://cluster.local/ns/gavel/sa/gavel-processes".to_owned(),
    });
    assert!(matches!(
        service
            .expire_payment(expire)
            .await
            .unwrap()
            .into_inner()
            .outcome
            .unwrap(),
        expire_payment_response::Outcome::Expired(_)
    ));
}

#[tokio::test]
async fn grpc_process_read_rejects_spoofing_and_maps_safe_typed_outcomes() {
    let (app, uow, _, _) = app(true, vec![]);
    insert_process_attempt(
        &uow,
        process_attempt(
            "attempt-grpc-read",
            "auction-grpc-read",
            2,
            PaymentAttemptState::RecoveryRequired,
        ),
    )
    .await;
    let allowed = "spiffe://cluster.local/ns/gavel/sa/gavel-processes";
    let service = GrpcPayments::new(app, allowed.to_owned(), std::time::Duration::from_secs(15));
    let message = rpc::GetSettlementAttemptRequest {
        payment_attempt_id: "attempt-grpc-read".to_owned(),
        expected_auction_id: "auction-grpc-read".to_owned(),
        expected_settlement_round: 2,
        correlation_id: "correlation-read".to_owned(),
        consistency_token: "authz-token".to_owned(),
    };

    let mut spoofed = tonic_bsr::Request::new(message.clone());
    spoofed
        .metadata_mut()
        .insert("x-spiffe-id", allowed.parse().unwrap());
    spoofed
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    assert_eq!(
        service
            .get_settlement_attempt(spoofed)
            .await
            .unwrap_err()
            .code(),
        tonic_bsr::Code::Unauthenticated
    );

    let mut wrong_workload = tonic_bsr::Request::new(message.clone());
    wrong_workload
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    wrong_workload
        .extensions_mut()
        .insert(AuthenticatedWorkload {
            spiffe_id: "spiffe://cluster.local/ns/other/sa/other".to_owned(),
        });
    assert_eq!(
        service
            .get_settlement_attempt(wrong_workload)
            .await
            .unwrap_err()
            .code(),
        tonic_bsr::Code::PermissionDenied
    );

    let mut excessive_deadline = tonic_bsr::Request::new(message.clone());
    excessive_deadline
        .metadata_mut()
        .insert("grpc-timeout", "16S".parse().unwrap());
    excessive_deadline
        .extensions_mut()
        .insert(AuthenticatedWorkload {
            spiffe_id: allowed.to_owned(),
        });
    assert_eq!(
        service
            .get_settlement_attempt(excessive_deadline)
            .await
            .unwrap_err()
            .code(),
        tonic_bsr::Code::InvalidArgument
    );

    let mut request = tonic_bsr::Request::new(message.clone());
    request
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    request.extensions_mut().insert(AuthenticatedWorkload {
        spiffe_id: allowed.to_owned(),
    });
    let result = match service
        .get_settlement_attempt(request)
        .await
        .unwrap()
        .into_inner()
        .outcome
        .unwrap()
    {
        get_settlement_attempt_response::Outcome::Result(result) => result,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert_eq!(result.candidate_id, "user-1");
    assert_eq!(
        result.status,
        rpc::PaymentAttemptStatus::RecoveryRequired as i32
    );
    assert!(!format!("{result:?}").contains("operation-secret"));
    assert!(matches!(
        result.state_data,
        Some(
            rpc::settlement_attempt_result::StateData::RecoveryRequired(
                rpc::SettlementRecoveryData { ref recovery_url }
            )
        ) if recovery_url == "https://provider.example/recover"
    ));

    let mut stale = tonic_bsr::Request::new(rpc::GetSettlementAttemptRequest {
        expected_settlement_round: 1,
        ..message
    });
    stale
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    stale.extensions_mut().insert(AuthenticatedWorkload {
        spiffe_id: allowed.to_owned(),
    });
    assert!(matches!(
        service
            .get_settlement_attempt(stale)
            .await
            .unwrap()
            .into_inner()
            .outcome,
        Some(get_settlement_attempt_response::Outcome::Rejection(
            rpc::DomainRejection { code, .. }
        )) if code == rpc::PaymentRejectionCode::InvalidState as i32
    ));
}

#[tokio::test]
async fn verified_callback_is_monotonic_replayed_once_and_conflicts_on_changed_body() {
    let (app, uow, _, callbacks) = app(true, vec![]);
    let instrument = PaymentInstrument::pending(
        PaymentInstrumentId::parse("instrument-callback").unwrap(),
        PrincipalId::parse("user-1").unwrap(),
        EmailAddress::parse("owner@example.test").unwrap(),
        DebitAuthorization::new(TermsVersion::parse("2026-10").unwrap(), 1),
        ProviderReference::parse("session-callback").unwrap(),
        1,
    );
    uow.save_instrument(
        Commit {
            value: instrument,
            events: vec![],
        },
        None,
    )
    .await
    .unwrap();
    let event = VerifiedProviderEvent {
        provider: "stripe",
        event_ref: ProviderReference::parse("event-callback").unwrap(),
        event_ref_digest: "event-digest".to_owned(),
        fingerprint: "body-a".to_owned(),
        occurred_at_millis: 2,
        outcome: VerifiedProviderOutcome::InstrumentReady {
            session_ref: ProviderReference::parse("session-callback").unwrap(),
            customer_ref: ProviderReference::parse("customer-callback").unwrap(),
            instrument_ref: ProviderReference::parse("method-callback").unwrap(),
            display: RedactedDisplay::parse("card").unwrap(),
        },
    };
    callbacks.1.store(1, Ordering::SeqCst);
    assert_eq!(
        app.handle_provider_event(event.clone()).await.unwrap_err(),
        ApplicationError::DependencyUnavailable
    );
    callbacks
        .0
        .lock()
        .unwrap()
        .get_mut(&("stripe".to_owned(), "event-digest".to_owned()))
        .unwrap()
        .2 = 0;
    app.handle_provider_event(event.clone()).await.unwrap();
    app.handle_provider_event(event.clone()).await.unwrap();
    {
        let state = uow.0.lock().unwrap();
        assert_eq!(state.events.len(), 1);
        assert_eq!(
            state.instruments["instrument-callback"].state(),
            PaymentInstrumentState::Ready
        );
    }
    let mut conflict = event;
    conflict.fingerprint = "body-b".to_owned();
    assert_eq!(
        app.handle_provider_event(conflict).await.unwrap_err(),
        ApplicationError::IdempotencyConflict
    );
}

#[tokio::test]
async fn provider_uncertainty_retries_same_attempt_and_emits_started_once() {
    let outcomes = vec![
        Err(PortError::Timeout),
        Ok(ProviderOutcome::Succeeded {
            operation_ref: ProviderReference::parse("operation-retry").unwrap(),
        }),
    ];
    let (app, uow, provider, _) = app(true, outcomes);
    let mut instrument = PaymentInstrument::pending(
        PaymentInstrumentId::parse("instrument-retry").unwrap(),
        PrincipalId::parse("user-1").unwrap(),
        EmailAddress::parse("owner@example.test").unwrap(),
        DebitAuthorization::new(TermsVersion::parse("2026-10").unwrap(), 1),
        ProviderReference::parse("session-retry").unwrap(),
        1,
    );
    instrument
        .mark_ready(
            ProviderReference::parse("customer-retry").unwrap(),
            ProviderReference::parse("method-retry").unwrap(),
            RedactedDisplay::parse("card").unwrap(),
            2,
        )
        .unwrap();
    uow.save_instrument(
        Commit {
            value: instrument,
            events: vec![],
        },
        None,
    )
    .await
    .unwrap();
    let command = AttemptPaymentCommand {
        workload_id: "spiffe-processes".to_owned(),
        auction_id: "auction-retry".to_owned(),
        settlement_round: 1,
        candidate_id: "user-1".to_owned(),
        amount_minor_units: 100,
        currency_code: "USD".to_owned(),
        idempotency_key: "attempt-retry".to_owned(),
        consistency_token: None,
    };
    assert_eq!(
        app.attempt_payment(command.clone()).await.unwrap_err(),
        ApplicationError::DependencyUnavailable
    );
    assert_eq!(
        app.attempt_payment(command).await.unwrap().status,
        PaymentAttemptState::Succeeded
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    let state = uow.0.lock().unwrap();
    assert_eq!(
        state
            .events
            .iter()
            .filter(|event| matches!(event, DomainEvent::PaymentAttemptStarted { .. }))
            .count(),
        1
    );
    assert_eq!(
        state
            .events
            .iter()
            .filter(|event| matches!(event, DomainEvent::PaymentSucceeded { .. }))
            .count(),
        1
    );
}

async fn insert_ready_instrument(uow: &MemoryUow, suffix: &str, owner: &str) {
    let mut instrument = PaymentInstrument::pending(
        PaymentInstrumentId::parse(format!("instrument-{suffix}")).unwrap(),
        PrincipalId::parse(owner).unwrap(),
        EmailAddress::parse("owner@example.test").unwrap(),
        DebitAuthorization::new(TermsVersion::parse("2026-10").unwrap(), 1),
        ProviderReference::parse(format!("session-{suffix}")).unwrap(),
        1,
    );
    instrument
        .mark_ready(
            ProviderReference::parse(format!("customer-{suffix}")).unwrap(),
            ProviderReference::parse(format!("method-{suffix}")).unwrap(),
            RedactedDisplay::parse("Visa •••• 4242").unwrap(),
            2,
        )
        .unwrap();
    uow.save_instrument(
        Commit {
            value: instrument,
            events: vec![],
        },
        None,
    )
    .await
    .unwrap();
}

fn attempt_command(suffix: &str) -> AttemptPaymentCommand {
    AttemptPaymentCommand {
        workload_id: "spiffe-processes".to_owned(),
        auction_id: format!("auction-{suffix}"),
        settlement_round: 1,
        candidate_id: "user-1".to_owned(),
        amount_minor_units: 100,
        currency_code: "USD".to_owned(),
        idempotency_key: format!("attempt-{suffix}"),
        consistency_token: None,
    }
}

fn process_attempt(
    attempt_id: &str,
    auction_id: &str,
    round: u32,
    state: PaymentAttemptState,
) -> PaymentAttempt {
    let mut attempt = PaymentAttempt::pending(
        PaymentAttemptId::parse(attempt_id).unwrap(),
        AuctionId::parse(auction_id).unwrap(),
        SettlementRound::new(round).unwrap(),
        CandidateId::parse("user-1").unwrap(),
        Money::new(100, Currency::USD).unwrap(),
        1,
    );
    match state {
        PaymentAttemptState::Pending => {}
        PaymentAttemptState::Processing => {
            attempt
                .mark_processing(ProviderReference::parse("operation-secret").unwrap(), 2)
                .unwrap();
        }
        PaymentAttemptState::RecoveryRequired => {
            attempt
                .require_recovery(
                    ProviderReference::parse("operation-secret").unwrap(),
                    "https://provider.example/recover".to_owned(),
                    2,
                )
                .unwrap();
        }
        PaymentAttemptState::Succeeded => {
            attempt
                .succeed(ProviderReference::parse("operation-secret").unwrap(), 2)
                .unwrap();
        }
        PaymentAttemptState::Expired => {
            attempt
                .mark_processing(ProviderReference::parse("operation-secret").unwrap(), 2)
                .unwrap();
            attempt.expire(3).unwrap();
        }
    }
    attempt
}

async fn insert_process_attempt(uow: &MemoryUow, attempt: PaymentAttempt) {
    uow.save_attempt(
        Commit {
            value: attempt,
            events: vec![],
        },
        None,
    )
    .await
    .unwrap();
}

fn process_query(attempt_id: &str, auction_id: &str, round: u32) -> GetSettlementAttemptQuery {
    GetSettlementAttemptQuery {
        workload_id: "spiffe-processes".to_owned(),
        attempt_id: attempt_id.to_owned(),
        expected_auction_id: auction_id.to_owned(),
        expected_settlement_round: round,
        consistency_token: Some("authz-token".to_owned()),
    }
}

#[tokio::test]
async fn process_read_is_authoritative_bound_and_state_dependent() {
    let (app, uow, _, _) = app(true, vec![]);
    insert_process_attempt(
        &uow,
        process_attempt(
            "attempt-process-recovery",
            "auction-process",
            2,
            PaymentAttemptState::RecoveryRequired,
        ),
    )
    .await;

    let view = app
        .get_settlement_attempt(process_query(
            "attempt-process-recovery",
            "auction-process",
            2,
        ))
        .await
        .unwrap();
    assert_eq!(view.status, PaymentAttemptState::RecoveryRequired);
    assert_eq!(view.candidate_id, "user-1");
    assert_eq!(view.checked_at_token, "token-1");
    assert_eq!(
        view.state_data,
        SettlementAttemptData::RecoveryRequired {
            recovery_url: "https://provider.example/recover".to_owned()
        }
    );
    let serialized = serde_json::to_string(&view).unwrap();
    assert!(serialized.contains("recovery_url"));
    assert!(!serialized.contains("operation-secret"));
    assert!(!serialized.contains("amount"));
}

#[tokio::test]
async fn process_read_rejects_denied_mismatched_stale_and_missing_requests() {
    let (denied, _, _, _) = app(false, vec![]);
    assert_eq!(
        denied
            .get_settlement_attempt(process_query("missing", "auction-process", 2))
            .await
            .unwrap_err(),
        ApplicationError::PermissionDenied
    );

    let (app, uow, _, _) = app(true, vec![]);
    insert_process_attempt(
        &uow,
        process_attempt(
            "attempt-process-binding",
            "auction-process",
            2,
            PaymentAttemptState::Processing,
        ),
    )
    .await;
    for query in [
        process_query("attempt-process-binding", "auction-other", 2),
        process_query("attempt-process-binding", "auction-process", 1),
    ] {
        assert_eq!(
            app.get_settlement_attempt(query).await.unwrap_err(),
            ApplicationError::InvalidState
        );
    }
    assert_eq!(
        app.get_settlement_attempt(process_query("missing", "auction-process", 2))
            .await
            .unwrap_err(),
        ApplicationError::NotFound
    );
}

#[tokio::test]
async fn process_read_exposes_payment_id_only_after_success() {
    let (app, uow, _, _) = app(true, vec![]);
    insert_process_attempt(
        &uow,
        process_attempt(
            "attempt-process-success",
            "auction-success",
            1,
            PaymentAttemptState::Succeeded,
        ),
    )
    .await;
    let view = app
        .get_settlement_attempt(process_query(
            "attempt-process-success",
            "auction-success",
            1,
        ))
        .await
        .unwrap();
    assert_eq!(
        view.state_data,
        SettlementAttemptData::Succeeded {
            payment_id: "attempt-process-success".to_owned()
        }
    );
    let serialized = serde_json::to_string(&view).unwrap();
    assert!(!serialized.contains("recovery_url"));
    assert!(!serialized.contains("operation-secret"));
}

#[tokio::test]
async fn stale_reconciliation_is_bounded_and_idempotent() {
    let outcomes = vec![
        Ok(ProviderOutcome::Processing {
            operation_ref: ProviderReference::parse("operation-reconcile").unwrap(),
        }),
        Ok(ProviderOutcome::Succeeded {
            operation_ref: ProviderReference::parse("operation-reconcile").unwrap(),
        }),
    ];
    let (app, uow, provider, _) = app(true, outcomes);
    insert_ready_instrument(&uow, "reconcile", "user-1").await;
    assert_eq!(
        app.attempt_payment(attempt_command("reconcile"))
            .await
            .unwrap()
            .status,
        PaymentAttemptState::Processing
    );
    assert_eq!(app.reconcile_stale_attempts(10_000, 1).await.unwrap(), 1);
    assert_eq!(app.reconcile_stale_attempts(10_000, 1).await.unwrap(), 0);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    let state = uow.0.lock().unwrap();
    assert_eq!(
        state
            .events
            .iter()
            .filter(|event| matches!(event, DomainEvent::PaymentSucceeded { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn expiration_retries_uncertain_cancel_and_replays_terminal_result() {
    let outcomes = vec![
        Ok(ProviderOutcome::Processing {
            operation_ref: ProviderReference::parse("operation-expire").unwrap(),
        }),
        Err(PortError::Timeout),
        Ok(ProviderOutcome::Declined),
    ];
    let (app, uow, provider, _) = app(true, outcomes);
    insert_ready_instrument(&uow, "expire", "user-1").await;
    let attempt = app
        .attempt_payment(attempt_command("expire"))
        .await
        .unwrap();
    let command = ExpirePaymentCommand {
        workload_id: "spiffe-processes".to_owned(),
        attempt_id: attempt.id,
        idempotency_key: "expire-retry".to_owned(),
        consistency_token: None,
    };
    assert_eq!(
        app.expire_payment(command.clone()).await.unwrap_err(),
        ApplicationError::DependencyUnavailable
    );
    let expired = app.expire_payment(command.clone()).await.unwrap();
    assert_eq!(expired.status, PaymentAttemptState::Expired);
    assert_eq!(app.expire_payment(command).await.unwrap(), expired);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn provider_success_wins_expiry_race() {
    let outcomes = vec![
        Ok(ProviderOutcome::Processing {
            operation_ref: ProviderReference::parse("operation-race").unwrap(),
        }),
        Ok(ProviderOutcome::Succeeded {
            operation_ref: ProviderReference::parse("operation-race").unwrap(),
        }),
    ];
    let (app, uow, provider, _) = app(true, outcomes);
    insert_ready_instrument(&uow, "race", "user-1").await;
    let attempt = app.attempt_payment(attempt_command("race")).await.unwrap();
    let result = app
        .expire_payment(ExpirePaymentCommand {
            workload_id: "spiffe-processes".to_owned(),
            attempt_id: attempt.id,
            idempotency_key: "expire-race".to_owned(),
            consistency_token: None,
        })
        .await
        .unwrap();
    assert_eq!(result.status, PaymentAttemptState::Succeeded);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}
