use gavel_processes::workflows::{
    AuctionId, CandidateId, PaymentAttemptId, SettlementAttemptStatus, SettlementDisposition,
    SettlementPhase, SettlementSignal, SettlementState,
};

fn apply_fixture(state: &mut SettlementState, fixture: &str) {
    for signal in serde_json::from_str::<Vec<SettlementSignal>>(fixture).unwrap() {
        let (disposition, _) = state.apply_signal(signal);
        assert_eq!(disposition, SettlementDisposition::Applied);
    }
}

#[test]
fn representative_success_recovery_and_restart_history_is_deterministic() {
    let fixture = include_str!("fixtures/settlement_success.json");
    let mut state = SettlementState::new(AuctionId::parse("auction-1").unwrap());
    let signals = serde_json::from_str::<Vec<SettlementSignal>>(fixture).unwrap();
    state.apply_signal(signals[0].clone());
    state
        .bind_attempt(
            SettlementAttemptStatus::RecoveryRequired,
            PaymentAttemptId::parse("attempt-1").unwrap(),
            CandidateId::parse("candidate-1").unwrap(),
            1,
            10_000,
        )
        .unwrap();
    let checkpoint = serde_json::to_string(&state).unwrap();
    let mut restarted: SettlementState = serde_json::from_str(&checkpoint).unwrap();

    for value in [&mut state, &mut restarted] {
        value.apply_signal(signals[1].clone());
        value
            .bind_attempt(
                SettlementAttemptStatus::Succeeded {
                    payment_id: "payment-1".to_owned(),
                },
                PaymentAttemptId::parse("attempt-1").unwrap(),
                CandidateId::parse("candidate-1").unwrap(),
                2,
                11_000,
            )
            .unwrap();
        value.winner_confirmed();
    }

    assert_eq!(state, restarted);
    assert_eq!(
        state.query().phase,
        SettlementPhase::Terminal(gavel_processes::workflows::SettlementTerminal::Sold)
    );
}

#[test]
fn representative_expiry_forfeiture_next_round_and_unsold_history_replays() {
    let success = serde_json::from_str::<Vec<SettlementSignal>>(include_str!(
        "fixtures/settlement_success.json"
    ))
    .unwrap();
    let mut state = SettlementState::new(AuctionId::parse("auction-1").unwrap());
    state.apply_signal(success[0].clone());
    state
        .bind_attempt(
            SettlementAttemptStatus::Expired,
            PaymentAttemptId::parse("attempt-1").unwrap(),
            CandidateId::parse("candidate-1").unwrap(),
            1,
            10_000,
        )
        .unwrap();
    state.forfeit_confirmed();
    apply_fixture(
        &mut state,
        include_str!("fixtures/settlement_next_round.json"),
    );

    assert_eq!(
        state.query().phase,
        SettlementPhase::Terminal(gavel_processes::workflows::SettlementTerminal::Unsold)
    );
}

#[test]
fn saved_history_has_no_recovery_url_provider_reference_or_maximum() {
    for fixture in [
        include_str!("fixtures/settlement_success.json"),
        include_str!("fixtures/settlement_next_round.json"),
    ] {
        let lower = fixture.to_ascii_lowercase();
        for forbidden in [
            "recovery_url",
            "https://",
            "provider_reference",
            "maximum",
            "max_bid",
        ] {
            assert!(!lower.contains(forbidden), "history contains {forbidden}");
        }
    }
}
