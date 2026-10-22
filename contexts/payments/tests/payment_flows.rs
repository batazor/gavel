use async_trait::async_trait;
use axum::{body::Body, http::Request};
use gavel_payments::rpc::{
    self, attempt_payment_response, expire_payment_response,
    payments_service_server::PaymentsService,
};
use gavel_payments::{
    adapters::{
        inbound::{
            AuthenticatedWorkload, CallbackIngress, GrpcPayments, StripeCallbackSink,
            callback_router,
        },
        outbound::{PostgresPayments, StripeConfig, StripeSignatureVerifier, SystemClock, UuidIds},
    },
    application::{
        Authorization, AuthorizationDecision, AuthorizationRequest, CallbackInbox, Clock,
        CommandFingerprint, Commit, IdGenerator, PaymentApplication, PaymentsUnitOfWork, PortError,
        ProviderGateway, ProviderOutcome, ReceiptExecutor, Telemetry,
    },
    domain::{
        AuctionId, CandidateId, Currency, DomainEvent, Money, PaymentAttempt, PaymentAttemptId,
        ProviderReference, SettlementRound,
    },
};
use sqlx::postgres::PgPoolOptions;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn database_enforces_one_attempt_per_round_and_one_success_per_auction() {
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    PostgresPayments::new(pool.clone()).migrate().await.unwrap();
    let payload = serde_json::json!({"test": true});
    sqlx::query("INSERT INTO payment_attempts(attempt_id,auction_id,settlement_round,candidate_id,state,version,aggregate) VALUES('a1','auction-x',1,'u1','succeeded',1,$1)").bind(&payload).execute(&pool).await.unwrap();
    assert!(sqlx::query("INSERT INTO payment_attempts(attempt_id,auction_id,settlement_round,candidate_id,state,version,aggregate) VALUES('a2','auction-x',1,'u2','processing',1,$1)").bind(&payload).execute(&pool).await.is_err());
    assert!(sqlx::query("INSERT INTO payment_attempts(attempt_id,auction_id,settlement_round,candidate_id,state,version,aggregate) VALUES('a3','auction-x',2,'u2','succeeded',1,$1)").bind(&payload).execute(&pool).await.is_err());
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn concurrent_success_and_expiry_have_one_commit_and_one_outbox_effect() {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let repository = Arc::new(PostgresPayments::new(pool.clone()));
    repository.migrate().await.unwrap();
    let suffix = uuid::Uuid::now_v7().simple().to_string();
    let mut initial = PaymentAttempt::pending(
        PaymentAttemptId::parse(format!("attempt-{suffix}")).unwrap(),
        AuctionId::parse(format!("auction-{suffix}")).unwrap(),
        SettlementRound::new(1).unwrap(),
        CandidateId::parse(format!("user-{suffix}")).unwrap(),
        Money::new(100, Currency::USD).unwrap(),
        1,
    );
    initial
        .mark_processing(
            ProviderReference::parse(format!("operation-{suffix}")).unwrap(),
            2,
        )
        .unwrap();
    repository
        .save_attempt(
            Commit {
                value: initial.clone(),
                events: vec![],
            },
            None,
        )
        .await
        .unwrap();
    let mut success = initial.clone();
    success
        .succeed(
            ProviderReference::parse(format!("operation-{suffix}")).unwrap(),
            3,
        )
        .unwrap();
    let success_event = DomainEvent::PaymentSucceeded {
        event_id: format!("success-{suffix}"),
        attempt_id: success.id().clone(),
        auction_id: success.auction_id().clone(),
        settlement_round: success.settlement_round(),
        version: success.version(),
        occurred_at_millis: 3,
    };
    let mut expired = initial.clone();
    expired.expire(3).unwrap();
    let expired_event = DomainEvent::PaymentExpired {
        event_id: format!("expired-{suffix}"),
        attempt_id: expired.id().clone(),
        auction_id: expired.auction_id().clone(),
        settlement_round: expired.settlement_round(),
        version: expired.version(),
        occurred_at_millis: 3,
    };
    let first = {
        let repository = repository.clone();
        tokio::spawn(async move {
            repository
                .save_attempt(
                    Commit {
                        value: success,
                        events: vec![success_event],
                    },
                    Some(2),
                )
                .await
        })
    };
    let second = {
        let repository = repository.clone();
        tokio::spawn(async move {
            repository
                .save_attempt(
                    Commit {
                        value: expired,
                        events: vec![expired_event],
                    },
                    Some(2),
                )
                .await
        })
    };
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let outbox: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payment_outbox WHERE event_id IN ($1,$2)")
            .bind(format!("success-{suffix}"))
            .bind(format!("expired-{suffix}"))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(outbox, 1);
}

struct AllowAuthz;
#[async_trait]
impl Authorization for AllowAuthz {
    async fn check(&self, _: AuthorizationRequest) -> Result<AuthorizationDecision, PortError> {
        Ok(AuthorizationDecision {
            allowed: true,
            checked_at_token: "integration-token".to_owned(),
        })
    }
    async fn touch_owner_grant(
        &self,
        _: &str,
        _: &gavel_payments::domain::PrincipalId,
        _: &str,
        _: u64,
    ) -> Result<String, PortError> {
        Ok("integration-token".to_owned())
    }
    async fn delete_owner_grant(
        &self,
        _: &str,
        _: &gavel_payments::domain::PrincipalId,
        _: &str,
        _: u64,
    ) -> Result<(), PortError> {
        Ok(())
    }
}

struct SequenceProvider {
    outcomes: Mutex<VecDeque<Result<ProviderOutcome, PortError>>>,
    calls: AtomicUsize,
}
#[async_trait]
impl ProviderGateway for SequenceProvider {
    async fn prepare(
        &self,
        _: gavel_payments::application::PrepareInstrumentRequest,
    ) -> Result<gavel_payments::application::PreparedInstrument, PortError> {
        Err(PortError::Internal)
    }
    async fn charge(
        &self,
        _: gavel_payments::application::ChargeRequest,
    ) -> Result<ProviderOutcome, PortError> {
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
impl SequenceProvider {
    fn new(outcomes: Vec<Result<ProviderOutcome, PortError>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            calls: AtomicUsize::new(0),
        }
    }
    fn next(&self) -> Result<ProviderOutcome, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes.lock().unwrap().pop_front().unwrap()
    }
}
struct NoopTelemetry;
impl Telemetry for NoopTelemetry {
    fn command(&self, _: &'static str, _: &'static str) {}
}

fn integration_application(
    repository: Arc<PostgresPayments>,
    provider: Arc<SequenceProvider>,
) -> Arc<PaymentApplication> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let ids: Arc<dyn IdGenerator> = Arc::new(UuidIds);
    Arc::new(PaymentApplication {
        uow: repository.clone(),
        provider,
        authz: Arc::new(AllowAuthz),
        clock: clock.clone(),
        ids,
        receipts: Arc::new(ReceiptExecutor::new(
            repository.clone(),
            clock,
            CommandFingerprint::new([7_u8; 32]).unwrap(),
            30_000,
        )),
        telemetry: Arc::new(NoopTelemetry),
        callbacks: repository as Arc<dyn CallbackInbox>,
    })
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn grpc_settlement_replays_across_restart_and_expires_same_provider_operation() {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let repository = Arc::new(PostgresPayments::new(pool));
    repository.migrate().await.unwrap();
    let suffix = uuid::Uuid::now_v7().simple().to_string();
    let owner = format!("user-{suffix}");
    let mut instrument = gavel_payments::domain::PaymentInstrument::pending(
        gavel_payments::domain::PaymentInstrumentId::parse(format!("instrument-{suffix}")).unwrap(),
        gavel_payments::domain::PrincipalId::parse(&owner).unwrap(),
        gavel_payments::domain::EmailAddress::parse(format!("owner-{suffix}@example.test"))
            .unwrap(),
        gavel_payments::domain::DebitAuthorization::new(
            gavel_payments::domain::TermsVersion::parse("2026-10").unwrap(),
            1,
        ),
        ProviderReference::parse(format!("session-{suffix}")).unwrap(),
        1,
    );
    instrument
        .mark_ready(
            ProviderReference::parse(format!("customer-{suffix}")).unwrap(),
            ProviderReference::parse(format!("method-{suffix}")).unwrap(),
            gavel_payments::domain::RedactedDisplay::parse("Visa •••• 4242").unwrap(),
            2,
        )
        .unwrap();
    repository
        .save_instrument(
            Commit {
                value: instrument,
                events: vec![],
            },
            None,
        )
        .await
        .unwrap();

    let operation = ProviderReference::parse(format!("operation-{suffix}")).unwrap();
    let first_provider = Arc::new(SequenceProvider::new(vec![Ok(
        ProviderOutcome::RecoveryRequired {
            operation_ref: operation.clone(),
            recovery_url: "https://provider.example/recover".to_owned(),
        },
    )]));
    let allowed = "spiffe://cluster.local/ns/gavel/sa/gavel-processes";
    let request_message = rpc::AttemptPaymentRequest {
        auction_id: format!("auction-{suffix}"),
        settlement_round: 1,
        candidate_id: owner,
        amount: Some(rpc::Money {
            minor_units: 100,
            currency_code: "USD".to_owned(),
        }),
        idempotency_key: format!("attempt-{suffix}"),
        correlation_id: format!("correlation-{suffix}"),
        consistency_token: String::new(),
    };
    let first_service = GrpcPayments::new(
        integration_application(repository.clone(), first_provider.clone()),
        allowed.to_owned(),
        Duration::from_secs(15),
    );
    let mut request = tonic_bsr::Request::new(request_message.clone());
    request
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    request.extensions_mut().insert(AuthenticatedWorkload {
        spiffe_id: allowed.to_owned(),
    });
    let attempt_id = match first_service
        .attempt_payment(request)
        .await
        .unwrap()
        .into_inner()
        .outcome
        .unwrap()
    {
        attempt_payment_response::Outcome::Result(result) => {
            assert_eq!(
                result.status,
                rpc::PaymentAttemptStatus::RecoveryRequired as i32
            );
            result.payment_attempt_id
        }
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert_eq!(first_provider.calls.load(Ordering::SeqCst), 1);

    let restarted_provider = Arc::new(SequenceProvider::new(vec![Ok(ProviderOutcome::Declined)]));
    let restarted = GrpcPayments::new(
        integration_application(repository.clone(), restarted_provider.clone()),
        allowed.to_owned(),
        Duration::from_secs(15),
    );
    let mut replay = tonic_bsr::Request::new(request_message);
    replay
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    replay.extensions_mut().insert(AuthenticatedWorkload {
        spiffe_id: allowed.to_owned(),
    });
    restarted.attempt_payment(replay).await.unwrap();
    assert_eq!(restarted_provider.calls.load(Ordering::SeqCst), 0);

    let mut expire = tonic_bsr::Request::new(rpc::ExpirePaymentRequest {
        payment_attempt_id: attempt_id,
        idempotency_key: format!("expire-{suffix}"),
        correlation_id: format!("correlation-expire-{suffix}"),
        consistency_token: String::new(),
    });
    expire
        .metadata_mut()
        .insert("grpc-timeout", "5S".parse().unwrap());
    expire.extensions_mut().insert(AuthenticatedWorkload {
        spiffe_id: allowed.to_owned(),
    });
    assert!(matches!(
        restarted
            .expire_payment(expire)
            .await
            .unwrap()
            .into_inner()
            .outcome
            .unwrap(),
        expire_payment_response::Outcome::Expired(_)
    ));
    assert_eq!(restarted_provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn signed_registration_callback_commits_one_delivery_safe_readiness_event() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let repository = Arc::new(PostgresPayments::new(pool));
    repository.migrate().await.unwrap();
    let suffix = uuid::Uuid::now_v7().simple().to_string();
    let session = format!("session-{suffix}");
    let pending = gavel_payments::domain::PaymentInstrument::pending(
        gavel_payments::domain::PaymentInstrumentId::parse(format!("instrument-{suffix}")).unwrap(),
        gavel_payments::domain::PrincipalId::parse(format!("user-{suffix}")).unwrap(),
        gavel_payments::domain::EmailAddress::parse(format!("owner-{suffix}@example.test"))
            .unwrap(),
        gavel_payments::domain::DebitAuthorization::new(
            gavel_payments::domain::TermsVersion::parse("2026-10").unwrap(),
            1,
        ),
        ProviderReference::parse(&session).unwrap(),
        1,
    );
    repository
        .save_instrument(
            Commit {
                value: pending,
                events: vec![],
            },
            None,
        )
        .await
        .unwrap();

    let application =
        integration_application(repository.clone(), Arc::new(SequenceProvider::new(vec![])));
    let callback_secret = "callback-secret-at-least-32-bytes";
    let stripe_config = StripeConfig::new(
        "https://provider.example/".parse().unwrap(),
        "provider-secret-long-enough".to_owned(),
        callback_secret.to_owned(),
        300,
    )
    .unwrap();
    let ingress = Arc::new(CallbackIngress {
        verifier: StripeSignatureVerifier::new(&stripe_config),
        sink: Arc::new(StripeCallbackSink::new(
            application,
            CommandFingerprint::new([7_u8; 32]).unwrap(),
        )),
        now_secs: Arc::new(|| 1_700_000_000),
    });
    let body = serde_json::json!({
        "id": format!("event-{suffix}"),
        "created": 1_700_000_000_u64,
        "type": "instrument.ready",
        "data": {"object": {
            "session_ref": session,
            "customer_ref": format!("customer-{suffix}"),
            "instrument_ref": format!("method-{suffix}"),
            "display": "Visa •••• 4242"
        }}
    })
    .to_string();
    let signed = format!("1700000000.{body}");
    let mut mac = Hmac::<Sha256>::new_from_slice(callback_secret.as_bytes()).unwrap();
    mac.update(signed.as_bytes());
    let signature = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    for _ in 0..2 {
        let response = callback_router(ingress.clone())
            .oneshot(
                Request::post("/payment-provider-callbacks/stripe")
                    .header("stripe-signature", format!("t=1700000000,v1={signature}"))
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }
    let payloads: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM payment_outbox WHERE event_type='PaymentInstrumentReady' AND payload->'payload'->>'userId'=$1",
    )
    .bind(format!("user-{suffix}"))
    .fetch_all(repository.pool())
    .await
    .unwrap();
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["eventType"], "PaymentInstrumentReady");
    assert_eq!(payloads[0]["schemaVersion"], 1);
    let serialized = payloads[0].to_string().to_lowercase();
    for forbidden in ["customer", "method", "session", "email", "4242"] {
        assert!(!serialized.contains(forbidden));
    }
}
