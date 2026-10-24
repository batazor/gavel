use gavel_payments::{
    adapters::outbound::{SandboxGateway, SandboxOutcome, sandbox_provider_key},
    application::{ChargeRequest, PrepareInstrumentRequest, ProviderGateway, ProviderOutcome},
    domain::ProviderReference,
};

fn charge(auction_id: &str, round: u32) -> ChargeRequest {
    ChargeRequest {
        provider_key: sandbox_provider_key(auction_id, round),
        customer_ref: ProviderReference::parse("sandbox-customer").unwrap(),
        instrument_ref: ProviderReference::parse("sandbox-instrument").unwrap(),
        amount_minor_units: 12_500,
        currency_code: "USD".to_owned(),
        timeout_millis: 8_000,
    }
}

#[tokio::test]
async fn sandbox_registration_and_success_are_deterministic() {
    let gateway = SandboxGateway::new(SandboxOutcome::Success);
    let request = PrepareInstrumentRequest {
        provider_key: "instrument:opaque-key".to_owned(),
        recovery_email: "bidder@example.test".to_owned(),
        timeout_millis: 5_000,
    };
    let first = gateway.prepare(request.clone()).await.unwrap();
    let replay = gateway.prepare(request).await.unwrap();
    assert_eq!(first, replay);

    assert!(matches!(
        gateway.charge(charge("auction-success", 1)).await.unwrap(),
        ProviderOutcome::Succeeded { .. }
    ));
}

#[tokio::test]
async fn accepted_operation_keeps_its_outcome_when_control_changes() {
    let gateway = SandboxGateway::new(SandboxOutcome::Success);
    gateway
        .set_outcome("auction-stable", 1, SandboxOutcome::RecoveryThenSuccess)
        .await;
    let first = gateway.charge(charge("auction-stable", 1)).await.unwrap();
    gateway
        .set_outcome("auction-stable", 1, SandboxOutcome::Success)
        .await;
    let retry = gateway.charge(charge("auction-stable", 1)).await.unwrap();

    assert_eq!(first, retry);
}

#[tokio::test]
async fn recovery_profile_uses_the_same_operation_then_succeeds() {
    let gateway = SandboxGateway::new(SandboxOutcome::Success);
    gateway
        .set_outcome("auction-recovery", 2, SandboxOutcome::RecoveryThenSuccess)
        .await;
    let outcome = gateway.charge(charge("auction-recovery", 2)).await.unwrap();
    let ProviderOutcome::RecoveryRequired {
        operation_ref,
        recovery_url,
    } = outcome
    else {
        panic!("recovery outcome expected");
    };
    assert!(recovery_url.starts_with("https://payments.local/"));
    assert!(matches!(
        gateway.continue_payment(&operation_ref, 8_000).await.unwrap(),
        ProviderOutcome::Succeeded { operation_ref: completed } if completed == operation_ref
    ));
}

#[tokio::test]
async fn expiry_profile_remains_recoverable_until_owner_expiry_query() {
    let gateway = SandboxGateway::new(SandboxOutcome::Success);
    gateway
        .set_outcome("auction-expiry", 3, SandboxOutcome::Expire)
        .await;
    let outcome = gateway.charge(charge("auction-expiry", 3)).await.unwrap();
    let ProviderOutcome::RecoveryRequired { operation_ref, .. } = outcome else {
        panic!("recovery outcome expected before expiry");
    };
    assert!(matches!(
        gateway
            .query_or_cancel(&operation_ref, 8_000)
            .await
            .unwrap(),
        ProviderOutcome::Declined
    ));
}
