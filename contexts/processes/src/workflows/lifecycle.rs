use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};
use temporalio_common::{RetryPolicy, search_attributes::SearchAttributeKey};
use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{
    ActivityOptions, ApplicationFailure, SyncWorkflowContext, WorkflowContext, WorkflowContextView,
    WorkflowResult,
};

use super::{AuctionId, AuctionLifecycleInput, CorrelationContext};
use crate::{
    activities::{
        CallContext, CloseAuctionActivityOutcome, CloseAuctionIntent, OwnerTemporalActivities,
    },
    rpc_policy::{ActivityPolicy, Intent, IntentKey},
};

const PROCESS_TYPE: SearchAttributeKey<String> = SearchAttributeKey::keyword("GavelProcessType");
const LIFECYCLE_PHASE: SearchAttributeKey<String> =
    SearchAttributeKey::keyword("GavelLifecyclePhase");
const CLOSE_DEADLINE_MILLIS: SearchAttributeKey<i64> =
    SearchAttributeKey::int("GavelCloseDeadlineMillis");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleTerminal {
    Sold,
    Unsold,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleEventKind {
    AuctionOpened { close_at_millis: u64 },
    DeadlineExtended { close_at_millis: u64 },
    AuctionRescheduled { close_at_millis: u64 },
    AuctionClosed,
    Terminal { outcome: LifecycleTerminal },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleSignal {
    pub event_id: String,
    pub fingerprint: String,
    pub aggregate_version: u64,
    pub kind: LifecycleEventKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecyclePhase {
    WaitingForDeadline,
    AwaitingOwnerDeadline,
    Closing,
    Terminal(LifecycleTerminal),
    InvariantViolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalDisposition {
    Applied,
    Duplicate,
    Stale,
    ChangedDuplicate,
    AlreadyTerminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerDecision {
    Wait(Duration),
    Due,
    Replaced,
    Complete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleState {
    auction_id: AuctionId,
    correlation: CorrelationContext,
    aggregate_version: u64,
    close_at_millis: u64,
    timer_generation: u64,
    phase: LifecyclePhase,
    seen_events: BTreeMap<String, String>,
    close_confirmed: bool,
}

impl LifecycleState {
    pub fn new(input: AuctionLifecycleInput) -> Self {
        Self {
            auction_id: input.auction_id,
            correlation: input.correlation,
            aggregate_version: input.opened_version,
            close_at_millis: input.close_at_millis,
            timer_generation: 1,
            phase: LifecyclePhase::WaitingForDeadline,
            seen_events: BTreeMap::new(),
            close_confirmed: false,
        }
    }

    pub fn apply(&mut self, signal: LifecycleSignal) -> SignalDisposition {
        if let Some(fingerprint) = self.seen_events.get(&signal.event_id) {
            if fingerprint == &signal.fingerprint {
                return SignalDisposition::Duplicate;
            }
            self.phase = LifecyclePhase::InvariantViolation;
            return SignalDisposition::ChangedDuplicate;
        }
        self.seen_events.insert(signal.event_id, signal.fingerprint);
        if matches!(self.phase, LifecyclePhase::Terminal(_)) {
            return SignalDisposition::AlreadyTerminal;
        }
        if signal.aggregate_version <= self.aggregate_version {
            return SignalDisposition::Stale;
        }
        self.aggregate_version = signal.aggregate_version;
        match signal.kind {
            LifecycleEventKind::AuctionOpened { close_at_millis }
            | LifecycleEventKind::DeadlineExtended { close_at_millis }
            | LifecycleEventKind::AuctionRescheduled { close_at_millis } => {
                self.close_at_millis = close_at_millis;
                self.timer_generation = self.timer_generation.saturating_add(1);
                self.phase = LifecyclePhase::WaitingForDeadline;
            }
            LifecycleEventKind::AuctionClosed => {
                self.close_confirmed = true;
                self.phase = LifecyclePhase::Closing;
            }
            LifecycleEventKind::Terminal { outcome } => {
                self.phase = LifecyclePhase::Terminal(outcome);
            }
        }
        SignalDisposition::Applied
    }

    pub fn query(&self) -> LifecycleQuery {
        LifecycleQuery {
            aggregate_version: self.aggregate_version,
            close_at_millis: self.close_at_millis,
            timer_generation: self.timer_generation,
            phase: self.phase,
            close_confirmed: self.close_confirmed,
        }
    }

    pub fn overdue_watchdog(&self, now_millis: u64) -> OverdueCloseWatchdog {
        OverdueCloseWatchdog {
            overdue: !matches!(self.phase, LifecyclePhase::Terminal(_))
                && now_millis > self.close_at_millis,
            deadline_millis: self.close_at_millis,
            phase: self.phase,
        }
    }

    pub fn metric_labels(&self) -> [(&'static str, &'static str); 2] {
        [
            ("workflow_type", "auction_lifecycle"),
            ("phase", phase_label(self.phase)),
        ]
    }

    pub fn timer_decision(&self, now_millis: u64, generation: u64) -> TimerDecision {
        if matches!(self.phase, LifecyclePhase::Terminal(_)) {
            return TimerDecision::Complete;
        }
        if self.timer_generation != generation {
            return TimerDecision::Replaced;
        }
        if now_millis >= self.close_at_millis {
            TimerDecision::Due
        } else {
            TimerDecision::Wait(Duration::from_millis(self.close_at_millis - now_millis))
        }
    }

    fn close_intent(&self, observed_at_millis: u64) -> Result<CloseAuctionIntent, String> {
        let key = IntentKey::for_auction(self.auction_id.as_str(), Intent::CloseAuction)
            .map_err(|error| error.to_string())?;
        Ok(CloseAuctionIntent {
            auction_id: self.auction_id.clone(),
            observed_at_millis,
            idempotency_key: key.to_string(),
            call: CallContext {
                correlation_id: self.correlation.correlation_id.clone(),
                causation_id: self.correlation.causation_id.clone(),
            },
        })
    }

    fn apply_close_outcome(&mut self, outcome: CloseAuctionActivityOutcome) {
        match outcome {
            CloseAuctionActivityOutcome::Closed(_) => {
                self.close_confirmed = true;
                self.phase = LifecyclePhase::Closing;
            }
            CloseAuctionActivityOutcome::Rejected { code }
                if matches!(code.as_str(), "sold" | "unsold" | "cancelled") =>
            {
                let terminal = match code.as_str() {
                    "sold" => LifecycleTerminal::Sold,
                    "unsold" => LifecycleTerminal::Unsold,
                    _ => LifecycleTerminal::Cancelled,
                };
                self.phase = LifecyclePhase::Terminal(terminal);
            }
            CloseAuctionActivityOutcome::Rejected { code } if code == "too_early" => {
                self.phase = LifecyclePhase::AwaitingOwnerDeadline;
            }
            CloseAuctionActivityOutcome::Rejected { .. } => {
                self.phase = LifecyclePhase::InvariantViolation;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleQuery {
    pub aggregate_version: u64,
    pub close_at_millis: u64,
    pub timer_generation: u64,
    pub phase: LifecyclePhase,
    pub close_confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverdueCloseWatchdog {
    pub overdue: bool,
    pub deadline_millis: u64,
    pub phase: LifecyclePhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleResult {
    pub terminal: LifecycleTerminal,
    pub aggregate_version: u64,
}

#[workflow]
pub struct AuctionLifecycleWorkflow {
    state: LifecycleState,
}

#[workflow_methods]
impl AuctionLifecycleWorkflow {
    #[init]
    pub fn new(_context: &WorkflowContextView, input: AuctionLifecycleInput) -> Self {
        Self {
            state: LifecycleState::new(input),
        }
    }

    #[run]
    pub async fn run(context: &mut WorkflowContext<Self>) -> WorkflowResult<LifecycleResult> {
        context.upsert_search_attributes([
            PROCESS_TYPE.value_set("auction_lifecycle".to_owned()),
            LIFECYCLE_PHASE.value_set("waiting_for_deadline".to_owned()),
            CLOSE_DEADLINE_MILLIS.value_set(clamp_i64(
                context.state(|value| value.state.close_at_millis),
            )),
        ]);
        loop {
            if let Some(result) = context.state(|value| match value.state.phase {
                LifecyclePhase::Terminal(terminal) => Some(LifecycleResult {
                    terminal,
                    aggregate_version: value.state.aggregate_version,
                }),
                _ => None,
            }) {
                return Ok(result);
            }
            if context.state(|value| value.state.phase == LifecyclePhase::InvariantViolation) {
                return Err(
                    ApplicationFailure::non_retryable("lifecycle invariant violated").into(),
                );
            }
            if context.state(|value| value.state.phase == LifecyclePhase::AwaitingOwnerDeadline) {
                let generation = context.state(|value| value.state.timer_generation);
                context
                    .wait_condition(move |value| {
                        value.state.timer_generation != generation
                            || matches!(value.state.phase, LifecyclePhase::Terminal(_))
                    })
                    .await?;
                continue;
            }
            let (deadline, generation) =
                context.state(|value| (value.state.close_at_millis, value.state.timer_generation));
            let now = workflow_time_millis(context).map_err(ApplicationFailure::non_retryable)?;
            if deadline > now {
                temporalio_sdk::workflows::select! {
                    _ = context.timer(Duration::from_millis(deadline - now)) => {}
                    _ = context.wait_condition(move |value| {
                        value.state.timer_generation != generation
                            || matches!(value.state.phase, LifecyclePhase::Terminal(_))
                    }) => { continue; }
                }
            }
            if context.state(|value| value.state.timer_generation != generation) {
                continue;
            }
            let observed_at =
                workflow_time_millis(context).map_err(ApplicationFailure::non_retryable)?;
            let intent = context
                .state(|value| value.state.close_intent(observed_at))
                .map_err(ApplicationFailure::non_retryable)?;
            let outcome = context
                .execute_activity(
                    OwnerTemporalActivities::close_auction,
                    intent,
                    lifecycle_activity_options(),
                )
                .await?;
            context.state_mut(|value| value.state.apply_close_outcome(outcome));
            context.upsert_search_attributes([
                PROCESS_TYPE.value_set("auction_lifecycle".to_owned()),
                LIFECYCLE_PHASE
                    .value_set(context.state(|value| phase_label(value.state.phase).to_owned())),
                CLOSE_DEADLINE_MILLIS.value_set(clamp_i64(
                    context.state(|value| value.state.close_at_millis),
                )),
            ]);
            if context.state(|value| value.state.close_confirmed) {
                let version = context.state(|value| value.state.aggregate_version);
                context
                    .wait_condition(move |value| {
                        value.state.aggregate_version > version
                            || matches!(value.state.phase, LifecyclePhase::Terminal(_))
                    })
                    .await?;
            }
        }
    }

    #[signal]
    pub fn owner_event(
        &mut self,
        _context: &mut SyncWorkflowContext<Self>,
        signal: LifecycleSignal,
    ) {
        self.state.apply(signal);
    }

    #[query]
    pub fn lifecycle_state(&self, _context: &WorkflowContextView) -> LifecycleQuery {
        self.state.query()
    }
}

fn workflow_time_millis(
    context: &WorkflowContext<AuctionLifecycleWorkflow>,
) -> Result<u64, &'static str> {
    let millis = context
        .workflow_time()
        .ok_or("workflow time is unavailable")?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "workflow time precedes epoch")?
        .as_millis();
    u64::try_from(millis).map_err(|_| "workflow time is out of range")
}

fn lifecycle_activity_options() -> ActivityOptions {
    let policy = ActivityPolicy::default();
    let retry = RetryPolicy::builder()
        .initial_interval(policy.retry.initial_interval)
        .backoff_coefficient(f64::from(policy.retry.backoff_coefficient))
        .maximum_interval(policy.retry.maximum_interval)
        .maximum_attempts(policy.retry.maximum_attempts)
        .non_retryable_error_types(["OwnerRejected", "InvalidInput", "InvariantViolation"])
        .build();
    ActivityOptions::with_start_to_close_timeout(policy.start_to_close_timeout)
        .retry_policy(retry)
        .build()
}

const fn phase_label(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::WaitingForDeadline => "waiting_for_deadline",
        LifecyclePhase::AwaitingOwnerDeadline => "awaiting_owner_deadline",
        LifecyclePhase::Closing => "closing",
        LifecyclePhase::Terminal(_) => "terminal",
        LifecyclePhase::InvariantViolation => "invariant_violation",
    }
}

fn clamp_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::ProcessId;

    fn state() -> LifecycleState {
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

    fn signal(event: &str, version: u64, deadline: u64) -> LifecycleSignal {
        LifecycleSignal {
            event_id: event.to_owned(),
            fingerprint: format!("fingerprint-{event}"),
            aggregate_version: version,
            kind: LifecycleEventKind::DeadlineExtended {
                close_at_millis: deadline,
            },
        }
    }

    #[test]
    fn transition_table_is_version_aware_and_detects_changed_redelivery() {
        let mut value = state();
        assert_eq!(
            value.apply(signal("extended-1", 3, 3_000)),
            SignalDisposition::Applied
        );
        assert_eq!(
            value.apply(signal("stale-1", 2, 2_000)),
            SignalDisposition::Stale
        );
        assert_eq!(
            value.apply(signal("extended-1", 3, 3_000)),
            SignalDisposition::Duplicate
        );
        let mut changed = signal("extended-1", 3, 4_000);
        changed.fingerprint = "changed".to_owned();
        assert_eq!(value.apply(changed), SignalDisposition::ChangedDuplicate);
        assert_eq!(value.phase, LifecyclePhase::InvariantViolation);
    }

    #[test]
    fn newer_deadline_logically_replaces_old_timer_generation() {
        let mut value = state();
        let old_generation = value.timer_generation;
        value.apply(signal("rescheduled-1", 2, 4_000));
        assert_eq!(value.close_at_millis, 4_000);
        assert!(value.timer_generation > old_generation);
        assert_eq!(
            value.timer_decision(1_000, old_generation),
            TimerDecision::Replaced
        );
        assert_eq!(
            value.timer_decision(1_000, value.timer_generation),
            TimerDecision::Wait(Duration::from_millis(3_000))
        );
    }

    #[test]
    fn close_keys_survive_restart_and_queries_have_only_safe_fields() {
        let value = state();
        let first = value.close_intent(1_000).unwrap();
        let replayed: LifecycleState =
            serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        let second = replayed.close_intent(1_000).unwrap();
        assert_eq!(first.idempotency_key, second.idempotency_key);
        assert_eq!(first.idempotency_key, "auction-lifecycle/auction-1:close");
        let query = serde_json::to_string(&value.query()).unwrap();
        assert!(!query.contains("auction-1"));
        assert!(!query.contains("correlation-1"));
        assert_eq!(
            value.metric_labels(),
            [
                ("workflow_type", "auction_lifecycle"),
                ("phase", "waiting_for_deadline")
            ]
        );
    }

    #[test]
    fn terminal_signal_completes_without_another_close_effect() {
        let mut value = state();
        assert_eq!(
            value.apply(LifecycleSignal {
                event_id: "cancelled-1".to_owned(),
                fingerprint: "terminal-fingerprint".to_owned(),
                aggregate_version: 2,
                kind: LifecycleEventKind::Terminal {
                    outcome: LifecycleTerminal::Cancelled
                },
            }),
            SignalDisposition::Applied
        );
        assert_eq!(
            value.phase,
            LifecyclePhase::Terminal(LifecycleTerminal::Cancelled)
        );
        assert_eq!(
            value.timer_decision(10_000, value.timer_generation),
            TimerDecision::Complete
        );
    }

    #[test]
    fn close_outcomes_follow_owner_without_local_domain_decisions() {
        let mut value = state();
        value.apply_close_outcome(CloseAuctionActivityOutcome::Rejected {
            code: "too_early".to_owned(),
        });
        assert_eq!(value.phase, LifecyclePhase::AwaitingOwnerDeadline);
        value.apply(signal("deadline-authoritative", 2, 2_000));
        assert_eq!(value.phase, LifecyclePhase::WaitingForDeadline);
        value.apply_close_outcome(CloseAuctionActivityOutcome::Rejected {
            code: "sold".to_owned(),
        });
        assert_eq!(
            value.phase,
            LifecyclePhase::Terminal(LifecycleTerminal::Sold)
        );
    }
}
