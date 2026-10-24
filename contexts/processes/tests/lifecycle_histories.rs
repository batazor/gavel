use gavel_processes::workflows::{
    AuctionId, AuctionLifecycleInput, CorrelationContext, LifecyclePhase, LifecycleSignal,
    LifecycleState, LifecycleTerminal, ProcessId, SignalDisposition,
};

fn initial() -> LifecycleState {
    LifecycleState::new(AuctionLifecycleInput {
        process_id: ProcessId::parse("auction-lifecycle/auction-1").unwrap(),
        auction_id: AuctionId::parse("auction-1").unwrap(),
        opened_version: 1,
        close_at_millis: 1_000,
        correlation: CorrelationContext {
            correlation_id: "correlation-1".to_owned(),
            causation_id: "opened-1".to_owned(),
        },
    })
}

fn replay(state: &mut LifecycleState, fixture: &str) {
    for signal in serde_json::from_str::<Vec<LifecycleSignal>>(fixture).unwrap() {
        assert_eq!(state.apply(signal), SignalDisposition::Applied);
    }
}

#[test]
fn representative_lifecycle_histories_replay_deterministically() {
    let mut first = initial();
    replay(
        &mut first,
        include_str!("fixtures/lifecycle_open_reschedule.json"),
    );
    let checkpoint = serde_json::to_string(&first).unwrap();

    let mut restarted: LifecycleState = serde_json::from_str(&checkpoint).unwrap();
    replay(&mut first, include_str!("fixtures/lifecycle_terminal.json"));
    replay(
        &mut restarted,
        include_str!("fixtures/lifecycle_terminal.json"),
    );

    assert_eq!(first, restarted);
    assert_eq!(first.query().aggregate_version, 4);
    assert_eq!(
        first.query().phase,
        LifecyclePhase::Terminal(LifecycleTerminal::Cancelled)
    );
    assert!(
        !serde_json::to_string(&first.query())
            .unwrap()
            .contains("auction-1")
    );
}

#[test]
fn too_early_owner_refresh_and_restart_history_replays() {
    let mut value = initial();
    replay(
        &mut value,
        include_str!("fixtures/lifecycle_open_reschedule.json"),
    );
    let checkpoint = serde_json::to_string(&value).unwrap();
    let mut restarted: LifecycleState = serde_json::from_str(&checkpoint).unwrap();
    replay(
        &mut value,
        include_str!("fixtures/lifecycle_too_early_restart.json"),
    );
    replay(
        &mut restarted,
        include_str!("fixtures/lifecycle_too_early_restart.json"),
    );

    assert_eq!(value, restarted);
    assert_eq!(value.query().close_at_millis, 9_000);
    assert_eq!(
        value.query().phase,
        LifecyclePhase::Terminal(LifecycleTerminal::Unsold)
    );
}
