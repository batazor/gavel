use gavel_payments::domain::*;

fn attempt() -> PaymentAttempt {
    PaymentAttempt::pending(
        PaymentAttemptId::parse("attempt-1").unwrap(),
        AuctionId::parse("auction-1").unwrap(),
        SettlementRound::new(1).unwrap(),
        CandidateId::parse("user-1").unwrap(),
        Money::new(12_500, Currency::USD).unwrap(),
        10,
    )
}

#[test]
fn attempt_transitions_are_monotonic_and_terminal() {
    let mut value = attempt();
    value
        .mark_processing(ProviderReference::parse("operation-1").unwrap(), 20)
        .unwrap();
    value
        .require_recovery(
            ProviderReference::parse("operation-1").unwrap(),
            "https://pay.example/recover".to_owned(),
            30,
        )
        .unwrap();
    value
        .succeed(ProviderReference::parse("operation-1").unwrap(), 40)
        .unwrap();
    assert_eq!(value.state(), PaymentAttemptState::Succeeded);
    assert_eq!(value.version(), 4);
    assert!(value.expire(50).is_err());
}

#[test]
fn expiry_is_terminal_and_success_cannot_arrive_after_commit() {
    let mut value = attempt();
    value
        .mark_processing(ProviderReference::parse("operation-2").unwrap(), 20)
        .unwrap();
    value.expire(30).unwrap();
    assert!(
        value
            .succeed(ProviderReference::parse("operation-2").unwrap(), 40)
            .is_err()
    );
}

#[test]
fn instrument_requires_pending_before_ready_and_redacts_secrets() {
    let customer = ProviderReference::parse("customer-secret").unwrap();
    assert!(!format!("{customer:?}").contains("customer-secret"));
    let mut value = PaymentInstrument::pending(
        PaymentInstrumentId::parse("instrument-1").unwrap(),
        PrincipalId::parse("user-1").unwrap(),
        EmailAddress::parse("owner@example.test").unwrap(),
        DebitAuthorization::new(TermsVersion::parse("2026-10").unwrap(), 10),
        ProviderReference::parse("session-1").unwrap(),
        10,
    );
    value
        .mark_ready(
            customer,
            ProviderReference::parse("instrument-ref").unwrap(),
            RedactedDisplay::parse("card").unwrap(),
            20,
        )
        .unwrap();
    assert!(
        value
            .mark_ready(
                ProviderReference::parse("c2").unwrap(),
                ProviderReference::parse("i2").unwrap(),
                RedactedDisplay::parse("card").unwrap(),
                30
            )
            .is_err()
    );
    value.revoke(40).unwrap();
    assert!(value.provider_customer_ref().is_none());
}

#[test]
fn value_objects_reject_invalid_or_zero_values() {
    assert!(Money::new(0, Currency::USD).is_err());
    assert!(SettlementRound::new(0).is_err());
    assert!("XYZ".parse::<Currency>().is_err());
    assert!(EmailAddress::parse("not-an-email").is_err());
    assert!(PaymentAttemptId::parse("contains space").is_err());
    assert!(RedactedDisplay::parse("").is_err());
    assert!(RedactedDisplay::parse("Visa •••• 4242").is_ok());
    assert!(RedactedDisplay::parse("4242 4242 4242 4242").is_err());
}

#[test]
fn instrument_only_allows_pending_ready_revoked_sequence() {
    let mut value = PaymentInstrument::pending(
        PaymentInstrumentId::parse("instrument-sequence").unwrap(),
        PrincipalId::parse("user-sequence").unwrap(),
        EmailAddress::parse("owner@example.test").unwrap(),
        DebitAuthorization::new(TermsVersion::parse("2026-10").unwrap(), 10),
        ProviderReference::parse("session-sequence").unwrap(),
        10,
    );
    assert_eq!(value.revoke(11), Err(DomainError::InvalidTransition));
    value
        .mark_ready(
            ProviderReference::parse("customer-sequence").unwrap(),
            ProviderReference::parse("method-sequence").unwrap(),
            RedactedDisplay::parse("Visa •••• 4242").unwrap(),
            12,
        )
        .unwrap();
    value.revoke(13).unwrap();
    assert_eq!(value.revoke(14), Err(DomainError::InvalidTransition));
}

#[test]
fn aggregate_version_overflow_is_rejected_without_state_change() {
    let json = serde_json::json!({
        "id":"attempt-overflow", "auction_id":"auction-1", "settlement_round":1,
        "candidate_id":"user-1", "amount":{"minor_units":100,"currency":"USD"},
        "state":"recovery_required", "provider_operation_ref":"operation-1",
        "recovery_url":"https://provider.example/recover", "version":u64::MAX,
        "updated_at_millis":1
    });
    let mut value: PaymentAttempt = serde_json::from_value(json).unwrap();
    assert_eq!(
        value.mark_processing(ProviderReference::parse("operation-1").unwrap(), 2),
        Err(DomainError::VersionOverflow)
    );
    assert_eq!(value.state(), PaymentAttemptState::RecoveryRequired);
}

#[test]
fn transition_table_rejects_every_terminal_outgoing_edge() {
    for state in ["succeeded", "expired"] {
        let json = serde_json::json!({
            "id":"attempt-terminal", "auction_id":"auction-1", "settlement_round":1,
            "candidate_id":"user-1", "amount":{"minor_units":100,"currency":"USD"},
            "state":state, "provider_operation_ref":"operation-1", "recovery_url":null,
            "version":4, "updated_at_millis":1
        });
        let mut value: PaymentAttempt = serde_json::from_value(json).unwrap();
        assert!(
            value
                .mark_processing(ProviderReference::parse("operation-1").unwrap(), 2)
                .is_err()
        );
        assert!(
            value
                .require_recovery(
                    ProviderReference::parse("operation-1").unwrap(),
                    "https://provider.example/recover".to_owned(),
                    2
                )
                .is_err()
        );
        assert!(
            value
                .succeed(ProviderReference::parse("operation-1").unwrap(), 2)
                .is_err()
        );
        assert!(value.expire(2).is_err());
    }
}
