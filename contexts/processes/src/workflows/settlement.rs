use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};
use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{
    ApplicationFailure, SyncWorkflowContext, WorkflowContext, WorkflowContextView, WorkflowResult,
};

use super::{
    AuctionId, CandidateId, CorrelationContext, Money, PaymentAttemptId, ProcessError,
    SettleAuctionInput, SettlementAttemptStatus, SettlementCandidate, SettlementRound,
};
use crate::{
    activities::{
        AttemptPaymentIntent, CallContext, ConfirmWinnerIntent, ExpirePaymentIntent,
        ForfeitCandidateIntent, NotificationKind, NotifyIntent, RecoveryNotificationIntent,
        SettlementAttemptRead, SettlementTemporalActivities,
    },
    adapters::owner_rpc::temporal_activity_options,
    rpc_policy::{Intent, IntentKey},
};

pub const RECOVERY_WINDOW_MILLIS: u64 = 2 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementTerminal {
    Sold,
    Unsold,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementPhase {
    AwaitingCandidate,
    AttemptPayment,
    WaitingForPayment,
    RecoveryRequired,
    ExpirePayment,
    ForfeitCandidate,
    AwaitingNextRound,
    ConfirmWinner,
    Terminal(SettlementTerminal),
    InvariantViolation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementOwnerSignal {
    CandidateSelected {
        candidate: SettlementCandidate,
    },
    PaymentChanged {
        attempt_id: PaymentAttemptId,
        round: SettlementRound,
        attempt_version: u64,
    },
    AuctionTerminal {
        outcome: SettlementTerminal,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementSignal {
    pub event_id: String,
    pub fingerprint: String,
    pub owner_version: u64,
    pub kind: SettlementOwnerSignal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementDisposition {
    Applied,
    Duplicate,
    Stale,
    ChangedDuplicate,
    WrongRound,
    AlreadyTerminal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementAction {
    None,
    AttemptPayment {
        round: SettlementRound,
    },
    ReadAuthoritativeAttempt {
        attempt_id: PaymentAttemptId,
        round: SettlementRound,
    },
    NotifyRecovery {
        attempt_id: PaymentAttemptId,
        round: SettlementRound,
    },
    ExpirePayment {
        attempt_id: PaymentAttemptId,
        round: SettlementRound,
    },
    ForfeitCandidate {
        candidate_id: CandidateId,
        round: SettlementRound,
    },
    ConfirmWinner {
        candidate_id: CandidateId,
        payment_id: String,
        round: SettlementRound,
    },
    Complete(SettlementTerminal),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CurrentRound {
    candidate_id: CandidateId,
    round: SettlementRound,
    amount: Money,
    auction_version: u64,
    attempt_id: Option<PaymentAttemptId>,
    attempt_version: u64,
    payment_status: Option<SettlementAttemptStatus>,
    recovery_deadline_millis: Option<u64>,
    confirm_requested: bool,
    forfeit_requested: bool,
    recovery_notification_attempted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementState {
    auction_id: AuctionId,
    phase: SettlementPhase,
    auction_version: u64,
    current: Option<CurrentRound>,
    seen_events: BTreeMap<String, String>,
    notification_failures: u32,
}

impl SettlementState {
    pub fn new(auction_id: AuctionId) -> Self {
        Self {
            auction_id,
            phase: SettlementPhase::AwaitingCandidate,
            auction_version: 0,
            current: None,
            seen_events: BTreeMap::new(),
            notification_failures: 0,
        }
    }

    pub fn apply_signal(
        &mut self,
        signal: SettlementSignal,
    ) -> (SettlementDisposition, SettlementAction) {
        if let Some(fingerprint) = self.seen_events.get(&signal.event_id) {
            if fingerprint == &signal.fingerprint {
                return (SettlementDisposition::Duplicate, SettlementAction::None);
            }
            self.phase = SettlementPhase::InvariantViolation;
            return (
                SettlementDisposition::ChangedDuplicate,
                SettlementAction::None,
            );
        }
        self.seen_events.insert(signal.event_id, signal.fingerprint);
        if matches!(self.phase, SettlementPhase::Terminal(_)) {
            return (
                SettlementDisposition::AlreadyTerminal,
                SettlementAction::None,
            );
        }
        match signal.kind {
            SettlementOwnerSignal::CandidateSelected { candidate } => {
                if candidate.auction_id != self.auction_id
                    || signal.owner_version != candidate.auction_version
                {
                    self.phase = SettlementPhase::InvariantViolation;
                    return (
                        SettlementDisposition::ChangedDuplicate,
                        SettlementAction::None,
                    );
                }
                if signal.owner_version <= self.auction_version {
                    return (SettlementDisposition::Stale, SettlementAction::None);
                }
                if self
                    .current
                    .as_ref()
                    .is_some_and(|current| candidate.round.get() <= current.round.get())
                {
                    return (SettlementDisposition::WrongRound, SettlementAction::None);
                }
                self.auction_version = candidate.auction_version;
                let round = candidate.round;
                self.current = Some(CurrentRound {
                    candidate_id: candidate.candidate_id,
                    round,
                    amount: candidate.amount,
                    auction_version: candidate.auction_version,
                    attempt_id: None,
                    attempt_version: 0,
                    payment_status: None,
                    recovery_deadline_millis: None,
                    confirm_requested: false,
                    forfeit_requested: false,
                    recovery_notification_attempted: false,
                });
                self.phase = SettlementPhase::AttemptPayment;
                (
                    SettlementDisposition::Applied,
                    SettlementAction::AttemptPayment { round },
                )
            }
            SettlementOwnerSignal::PaymentChanged {
                attempt_id,
                round,
                attempt_version,
            } => {
                let Some(current) = self.current.as_mut() else {
                    return (SettlementDisposition::WrongRound, SettlementAction::None);
                };
                if current.round != round {
                    return (SettlementDisposition::WrongRound, SettlementAction::None);
                }
                if let Some(bound) = &current.attempt_id
                    && bound != &attempt_id
                {
                    self.phase = SettlementPhase::InvariantViolation;
                    return (
                        SettlementDisposition::ChangedDuplicate,
                        SettlementAction::None,
                    );
                }
                if attempt_version <= current.attempt_version {
                    return (SettlementDisposition::Stale, SettlementAction::None);
                }
                current.attempt_id = Some(attempt_id.clone());
                current.attempt_version = attempt_version;
                (
                    SettlementDisposition::Applied,
                    SettlementAction::ReadAuthoritativeAttempt { attempt_id, round },
                )
            }
            SettlementOwnerSignal::AuctionTerminal { outcome } => {
                if signal.owner_version <= self.auction_version {
                    return (SettlementDisposition::Stale, SettlementAction::None);
                }
                self.auction_version = signal.owner_version;
                self.phase = SettlementPhase::Terminal(outcome);
                (
                    SettlementDisposition::Applied,
                    SettlementAction::Complete(outcome),
                )
            }
        }
    }

    pub fn bind_attempt(
        &mut self,
        snapshot: super::SettlementAttemptStatus,
        attempt_id: PaymentAttemptId,
        candidate_id: CandidateId,
        version: u64,
        now_millis: u64,
    ) -> Result<SettlementAction, ProcessError> {
        let current = self
            .current
            .as_mut()
            .ok_or(ProcessError::InvariantViolation)?;
        if current.candidate_id != candidate_id
            || current
                .attempt_id
                .as_ref()
                .is_some_and(|bound| bound != &attempt_id)
            || version < current.attempt_version
        {
            return Err(ProcessError::InvariantViolation);
        }
        current.attempt_id = Some(attempt_id.clone());
        current.attempt_version = version;
        current.payment_status = Some(snapshot.clone());
        match snapshot {
            SettlementAttemptStatus::Processing => {
                self.phase = SettlementPhase::WaitingForPayment;
                Ok(SettlementAction::None)
            }
            SettlementAttemptStatus::RecoveryRequired => {
                if current.recovery_deadline_millis.is_none() {
                    current.recovery_deadline_millis =
                        Some(now_millis.saturating_add(RECOVERY_WINDOW_MILLIS));
                }
                self.phase = SettlementPhase::RecoveryRequired;
                Ok(SettlementAction::NotifyRecovery {
                    attempt_id,
                    round: current.round,
                })
            }
            SettlementAttemptStatus::Succeeded { payment_id } => {
                if current.confirm_requested {
                    return Ok(SettlementAction::None);
                }
                current.confirm_requested = true;
                self.phase = SettlementPhase::ConfirmWinner;
                Ok(SettlementAction::ConfirmWinner {
                    candidate_id: current.candidate_id.clone(),
                    payment_id,
                    round: current.round,
                })
            }
            SettlementAttemptStatus::Expired => self.forfeit_current(),
        }
    }

    pub fn recovery_timer_fired(&mut self, now_millis: u64) -> SettlementAction {
        let Some(current) = self.current.as_ref() else {
            return SettlementAction::None;
        };
        if self.phase != SettlementPhase::RecoveryRequired
            || current
                .recovery_deadline_millis
                .is_none_or(|deadline| now_millis < deadline)
        {
            return SettlementAction::None;
        }
        SettlementAction::ReadAuthoritativeAttempt {
            attempt_id: current
                .attempt_id
                .clone()
                .expect("recovery attempt is bound"),
            round: current.round,
        }
    }

    pub fn authoritative_after_recovery_timeout(&mut self) -> SettlementAction {
        let Some(current) = self.current.as_ref() else {
            return SettlementAction::None;
        };
        match current.payment_status.as_ref() {
            Some(SettlementAttemptStatus::Succeeded { payment_id }) => {
                SettlementAction::ConfirmWinner {
                    candidate_id: current.candidate_id.clone(),
                    payment_id: payment_id.clone(),
                    round: current.round,
                }
            }
            Some(SettlementAttemptStatus::Expired) => {
                self.forfeit_current().unwrap_or(SettlementAction::None)
            }
            Some(SettlementAttemptStatus::RecoveryRequired) => {
                self.phase = SettlementPhase::ExpirePayment;
                SettlementAction::ExpirePayment {
                    attempt_id: current
                        .attempt_id
                        .clone()
                        .expect("recovery attempt is bound"),
                    round: current.round,
                }
            }
            _ => SettlementAction::None,
        }
    }

    pub fn forfeit_confirmed(&mut self) {
        self.phase = SettlementPhase::AwaitingNextRound;
    }

    pub fn winner_confirmed(&mut self) -> SettlementAction {
        self.phase = SettlementPhase::Terminal(SettlementTerminal::Sold);
        SettlementAction::Complete(SettlementTerminal::Sold)
    }

    pub fn record_notification_failure(&mut self) {
        self.notification_failures = self.notification_failures.saturating_add(1);
    }

    fn mark_recovery_notification_attempted(&mut self) {
        if let Some(current) = self.current.as_mut() {
            current.recovery_notification_attempted = true;
        }
    }

    fn current_context(&self) -> Result<RoundContext, ProcessError> {
        let current = self
            .current
            .as_ref()
            .ok_or(ProcessError::InvariantViolation)?;
        Ok(RoundContext {
            candidate_id: current.candidate_id.clone(),
            round: current.round,
            amount: current.amount.clone(),
            attempt_id: current.attempt_id.clone(),
            attempt_version: current.attempt_version,
            recovery_deadline_millis: current.recovery_deadline_millis,
            recovery_notification_attempted: current.recovery_notification_attempted,
        })
    }

    pub fn query(&self) -> SettlementQuery {
        SettlementQuery {
            phase: self.phase,
            round: self.current.as_ref().map(|current| current.round.get()),
            attempt_version: self
                .current
                .as_ref()
                .map_or(0, |current| current.attempt_version),
            recovery_deadline_millis: self
                .current
                .as_ref()
                .and_then(|current| current.recovery_deadline_millis),
            notification_failures: self.notification_failures,
        }
    }

    pub fn current_amount(&self) -> Option<&Money> {
        self.current.as_ref().map(|current| &current.amount)
    }

    fn forfeit_current(&mut self) -> Result<SettlementAction, ProcessError> {
        let current = self
            .current
            .as_mut()
            .ok_or(ProcessError::InvariantViolation)?;
        if current.forfeit_requested {
            return Ok(SettlementAction::None);
        }
        current.forfeit_requested = true;
        self.phase = SettlementPhase::ForfeitCandidate;
        Ok(SettlementAction::ForfeitCandidate {
            candidate_id: current.candidate_id.clone(),
            round: current.round,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RoundContext {
    candidate_id: CandidateId,
    round: SettlementRound,
    amount: Money,
    attempt_id: Option<PaymentAttemptId>,
    attempt_version: u64,
    recovery_deadline_millis: Option<u64>,
    recovery_notification_attempted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementQuery {
    pub phase: SettlementPhase,
    pub round: Option<u32>,
    pub attempt_version: u64,
    pub recovery_deadline_millis: Option<u64>,
    pub notification_failures: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementResult {
    pub terminal: SettlementTerminal,
    pub auction_version: u64,
}

#[workflow]
pub struct SettleAuctionWorkflow {
    state: SettlementState,
    correlation: CorrelationContext,
}

#[workflow_methods]
impl SettleAuctionWorkflow {
    #[init]
    pub fn new(_context: &WorkflowContextView, input: SettleAuctionInput) -> Self {
        Self {
            state: SettlementState::new(input.auction_id),
            correlation: input.correlation,
        }
    }

    #[run]
    pub async fn run(context: &mut WorkflowContext<Self>) -> WorkflowResult<SettlementResult> {
        loop {
            let phase = context.state(|workflow| workflow.state.phase);
            match phase {
                SettlementPhase::AwaitingCandidate | SettlementPhase::AwaitingNextRound => {
                    let version = context.state(|workflow| workflow.state.auction_version);
                    context
                        .wait_condition(move |workflow| {
                            workflow.state.auction_version > version
                                || matches!(workflow.state.phase, SettlementPhase::Terminal(_))
                        })
                        .await?;
                }
                SettlementPhase::AttemptPayment => {
                    let (auction_id, correlation, round) = context.state(|workflow| {
                        (
                            workflow.state.auction_id.clone(),
                            workflow.correlation.clone(),
                            workflow.state.current_context(),
                        )
                    });
                    let round = round.map_err(non_retryable_process_error)?;
                    let notify = NotifyIntent {
                        auction_id: auction_id.clone(),
                        round: round.round,
                        recipient_id: round.candidate_id.clone(),
                        kind: NotificationKind::SettlementCandidate,
                        correlation_id: correlation.correlation_id.clone(),
                    };
                    if context
                        .execute_activity(
                            SettlementTemporalActivities::notify,
                            notify,
                            temporal_activity_options(),
                        )
                        .await
                        .is_err()
                    {
                        context.state_mut(|workflow| workflow.state.record_notification_failure());
                    }
                    let key = stable_key(
                        auction_id.as_str(),
                        Intent::AttemptPayment {
                            round: round.round.get(),
                        },
                    )
                    .map_err(ApplicationFailure::non_retryable)?;
                    let snapshot = context
                        .execute_activity(
                            SettlementTemporalActivities::attempt_payment,
                            AttemptPaymentIntent {
                                auction_id,
                                round: round.round,
                                candidate_id: round.candidate_id.clone(),
                                amount: round.amount,
                                idempotency_key: key,
                                correlation_id: correlation.correlation_id,
                                consistency_token: String::new(),
                            },
                            temporal_activity_options(),
                        )
                        .await?;
                    let now =
                        workflow_time_millis(context).map_err(ApplicationFailure::non_retryable)?;
                    context
                        .state_mut(|workflow| {
                            workflow.state.bind_attempt(
                                snapshot.status,
                                snapshot.attempt_id,
                                snapshot.candidate_id,
                                snapshot.version,
                                now,
                            )
                        })
                        .map_err(non_retryable_process_error)?;
                }
                SettlementPhase::WaitingForPayment => {
                    let attempt_version = context.state(|workflow| {
                        workflow
                            .state
                            .current_context()
                            .map(|round| round.attempt_version)
                    });
                    let attempt_version = attempt_version.map_err(non_retryable_process_error)?;
                    context
                        .wait_condition(move |workflow| {
                            workflow
                                .state
                                .current
                                .as_ref()
                                .is_some_and(|round| round.attempt_version > attempt_version)
                                || matches!(workflow.state.phase, SettlementPhase::Terminal(_))
                        })
                        .await?;
                    if context.state(|workflow| {
                        matches!(workflow.state.phase, SettlementPhase::Terminal(_))
                    }) {
                        continue;
                    }
                    reconcile_attempt(context).await?;
                }
                SettlementPhase::RecoveryRequired => {
                    notify_recovery_once(context).await;
                    let round = context
                        .state(|workflow| workflow.state.current_context())
                        .map_err(non_retryable_process_error)?;
                    let deadline = round.recovery_deadline_millis.ok_or_else(|| {
                        ApplicationFailure::non_retryable("recovery deadline is missing")
                    })?;
                    let now =
                        workflow_time_millis(context).map_err(ApplicationFailure::non_retryable)?;
                    if deadline > now {
                        let version = round.attempt_version;
                        temporalio_sdk::workflows::select! {
                            _ = context.timer(Duration::from_millis(deadline - now)) => {}
                            _ = context.wait_condition(move |workflow| {
                                workflow.state.current.as_ref().is_some_and(|round| round.attempt_version > version)
                                    || matches!(workflow.state.phase, SettlementPhase::Terminal(_))
                            }) => {
                                if !context.state(|workflow| matches!(workflow.state.phase, SettlementPhase::Terminal(_))) {
                                    reconcile_attempt(context).await?;
                                }
                                continue;
                            }
                        }
                    }
                    reconcile_attempt(context).await?;
                    if context.state(|workflow| workflow.state.phase)
                        == SettlementPhase::RecoveryRequired
                    {
                        context.state_mut(|workflow| {
                            workflow.state.authoritative_after_recovery_timeout()
                        });
                    }
                }
                SettlementPhase::ExpirePayment => {
                    let (auction_id, correlation, round) = context.state(|workflow| {
                        (
                            workflow.state.auction_id.clone(),
                            workflow.correlation.clone(),
                            workflow.state.current_context(),
                        )
                    });
                    let round = round.map_err(non_retryable_process_error)?;
                    let attempt_id = round.attempt_id.ok_or_else(|| {
                        ApplicationFailure::non_retryable("payment attempt is missing")
                    })?;
                    let snapshot = context
                        .execute_activity(
                            SettlementTemporalActivities::expire_payment,
                            ExpirePaymentIntent {
                                attempt_id,
                                auction_id: auction_id.clone(),
                                round: round.round,
                                candidate_id: round.candidate_id.clone(),
                                idempotency_key: stable_key(
                                    auction_id.as_str(),
                                    Intent::ExpirePayment {
                                        round: round.round.get(),
                                    },
                                )
                                .map_err(ApplicationFailure::non_retryable)?,
                                correlation_id: correlation.correlation_id,
                                consistency_token: String::new(),
                            },
                            temporal_activity_options(),
                        )
                        .await?;
                    let now =
                        workflow_time_millis(context).map_err(ApplicationFailure::non_retryable)?;
                    context
                        .state_mut(|workflow| {
                            workflow.state.bind_attempt(
                                snapshot.status,
                                snapshot.attempt_id,
                                snapshot.candidate_id,
                                snapshot.version,
                                now,
                            )
                        })
                        .map_err(non_retryable_process_error)?;
                }
                SettlementPhase::ForfeitCandidate => {
                    let (auction_id, correlation, round) = context.state(|workflow| {
                        (
                            workflow.state.auction_id.clone(),
                            workflow.correlation.clone(),
                            workflow.state.current_context(),
                        )
                    });
                    let round = round.map_err(non_retryable_process_error)?;
                    let failed_candidate = round.candidate_id.clone();
                    context
                        .execute_activity(
                            SettlementTemporalActivities::forfeit_candidate,
                            ForfeitCandidateIntent {
                                auction_id: auction_id.clone(),
                                candidate_id: round.candidate_id,
                                round: round.round,
                                idempotency_key: stable_key(
                                    auction_id.as_str(),
                                    Intent::ForfeitSettlementCandidate {
                                        round: round.round.get(),
                                    },
                                )
                                .map_err(ApplicationFailure::non_retryable)?,
                                call: call_context(&correlation),
                            },
                            temporal_activity_options(),
                        )
                        .await?;
                    let failure_notice = NotifyIntent {
                        auction_id: auction_id.clone(),
                        round: round.round,
                        recipient_id: failed_candidate,
                        kind: NotificationKind::SettlementCandidateFailed,
                        correlation_id: correlation.correlation_id,
                    };
                    if context
                        .execute_activity(
                            SettlementTemporalActivities::notify,
                            failure_notice,
                            temporal_activity_options(),
                        )
                        .await
                        .is_err()
                    {
                        context.state_mut(|workflow| workflow.state.record_notification_failure());
                    }
                    context.state_mut(|workflow| workflow.state.forfeit_confirmed());
                }
                SettlementPhase::ConfirmWinner => {
                    let (auction_id, correlation, round) = context.state(|workflow| {
                        (
                            workflow.state.auction_id.clone(),
                            workflow.correlation.clone(),
                            workflow.state.current_context(),
                        )
                    });
                    let round = round.map_err(non_retryable_process_error)?;
                    let payment_id = context
                        .state(|workflow| {
                            workflow.state.current.as_ref().and_then(|current| {
                                match current.payment_status.as_ref() {
                                    Some(SettlementAttemptStatus::Succeeded { payment_id }) => {
                                        Some(payment_id.clone())
                                    }
                                    _ => None,
                                }
                            })
                        })
                        .ok_or_else(|| {
                            ApplicationFailure::non_retryable("successful payment is missing")
                        })?;
                    context
                        .execute_activity(
                            SettlementTemporalActivities::confirm_winner,
                            ConfirmWinnerIntent {
                                auction_id: auction_id.clone(),
                                candidate_id: round.candidate_id.clone(),
                                payment_id,
                                round: round.round,
                                idempotency_key: stable_key(
                                    auction_id.as_str(),
                                    Intent::ConfirmWinner {
                                        round: round.round.get(),
                                    },
                                )
                                .map_err(ApplicationFailure::non_retryable)?,
                                call: call_context(&correlation),
                            },
                            temporal_activity_options(),
                        )
                        .await?;
                    context.state_mut(|workflow| workflow.state.winner_confirmed());
                    let winner_notice = NotifyIntent {
                        auction_id,
                        round: round.round,
                        recipient_id: round.candidate_id,
                        kind: NotificationKind::WinnerConfirmed,
                        correlation_id: correlation.correlation_id,
                    };
                    if context
                        .execute_activity(
                            SettlementTemporalActivities::notify,
                            winner_notice,
                            temporal_activity_options(),
                        )
                        .await
                        .is_err()
                    {
                        context.state_mut(|workflow| workflow.state.record_notification_failure());
                    }
                }
                SettlementPhase::Terminal(terminal) => {
                    return Ok(SettlementResult {
                        terminal,
                        auction_version: context.state(|workflow| workflow.state.auction_version),
                    });
                }
                SettlementPhase::InvariantViolation => {
                    return Err(
                        ApplicationFailure::non_retryable("settlement invariant violated").into(),
                    );
                }
            }
        }
    }

    #[signal]
    pub fn owner_event(
        &mut self,
        _context: &mut SyncWorkflowContext<Self>,
        signal: SettlementSignal,
    ) {
        self.state.apply_signal(signal);
    }

    #[query]
    pub fn settlement_state(&self, _context: &WorkflowContextView) -> SettlementQuery {
        self.state.query()
    }
}

/// Typed workflow definition emitted by the Temporal macro, exposed for integration handles.
pub use settle_auction_workflow::Run as SettleAuctionRun;

async fn reconcile_attempt(
    context: &mut WorkflowContext<SettleAuctionWorkflow>,
) -> WorkflowResult<()> {
    let (auction_id, correlation, round) = context.state(|workflow| {
        (
            workflow.state.auction_id.clone(),
            workflow.correlation.clone(),
            workflow.state.current_context(),
        )
    });
    let round = round.map_err(non_retryable_process_error)?;
    let attempt_id = round
        .attempt_id
        .ok_or_else(|| ApplicationFailure::non_retryable("payment attempt is missing"))?;
    let snapshot = context
        .execute_activity(
            SettlementTemporalActivities::get_settlement_attempt,
            SettlementAttemptRead {
                attempt_id,
                expected_auction_id: auction_id,
                expected_round: round.round,
                correlation_id: correlation.correlation_id,
                consistency_token: String::new(),
            },
            temporal_activity_options(),
        )
        .await?;
    let now = workflow_time_millis(context).map_err(ApplicationFailure::non_retryable)?;
    context
        .state_mut(|workflow| {
            workflow.state.bind_attempt(
                snapshot.status,
                snapshot.attempt_id,
                snapshot.candidate_id,
                snapshot.version,
                now,
            )
        })
        .map_err(non_retryable_process_error)?;
    Ok(())
}

async fn notify_recovery_once(context: &mut WorkflowContext<SettleAuctionWorkflow>) {
    let (correlation, round, auction_id) = context.state(|workflow| {
        (
            workflow.correlation.clone(),
            workflow.state.current_context(),
            workflow.state.auction_id.clone(),
        )
    });
    let Ok(round) = round else {
        return;
    };
    if round.recovery_notification_attempted {
        return;
    }
    context.state_mut(|workflow| workflow.state.mark_recovery_notification_attempted());
    let Some(attempt_id) = round.attempt_id else {
        context.state_mut(|workflow| workflow.state.record_notification_failure());
        return;
    };
    let result = context
        .execute_activity(
            SettlementTemporalActivities::notify_recovery,
            RecoveryNotificationIntent {
                attempt: SettlementAttemptRead {
                    attempt_id,
                    expected_auction_id: auction_id,
                    expected_round: round.round,
                    correlation_id: correlation.correlation_id,
                    consistency_token: String::new(),
                },
                recipient_id: round.candidate_id,
            },
            temporal_activity_options(),
        )
        .await;
    if result.is_err() {
        context.state_mut(|workflow| workflow.state.record_notification_failure());
    }
}

fn stable_key(auction_id: &str, intent: Intent) -> Result<String, String> {
    IntentKey::for_auction(auction_id, intent)
        .map(|key| key.to_string())
        .map_err(|error| error.to_string())
}

fn call_context(correlation: &CorrelationContext) -> CallContext {
    CallContext {
        correlation_id: correlation.correlation_id.clone(),
        causation_id: correlation.causation_id.clone(),
    }
}

fn workflow_time_millis(
    context: &WorkflowContext<SettleAuctionWorkflow>,
) -> Result<u64, &'static str> {
    let millis = context
        .workflow_time()
        .ok_or("workflow time is unavailable")?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "workflow time precedes epoch")?
        .as_millis();
    u64::try_from(millis).map_err(|_| "workflow time is out of range")
}

fn non_retryable_process_error(error: ProcessError) -> ApplicationFailure {
    ApplicationFailure::non_retryable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(round: u32, version: u64, amount: u64) -> SettlementSignal {
        SettlementSignal {
            event_id: format!("candidate-{round}"),
            fingerprint: format!("fingerprint-{round}"),
            owner_version: version,
            kind: SettlementOwnerSignal::CandidateSelected {
                candidate: SettlementCandidate {
                    auction_id: AuctionId::parse("auction-1").unwrap(),
                    candidate_id: CandidateId::parse(format!("candidate-{round}")).unwrap(),
                    round: SettlementRound::new(round).unwrap(),
                    amount: Money::new(amount, "USD").unwrap(),
                    auction_version: version,
                },
            },
        }
    }

    fn state_with_attempt(status: SettlementAttemptStatus, now: u64) -> SettlementState {
        let mut state = SettlementState::new(AuctionId::parse("auction-1").unwrap());
        state.apply_signal(candidate(1, 1, 500));
        state
            .bind_attempt(
                status,
                PaymentAttemptId::parse("attempt-1").unwrap(),
                CandidateId::parse("candidate-1").unwrap(),
                1,
                now,
            )
            .unwrap();
        state
    }

    #[test]
    fn rounds_are_monotonic_and_changed_duplicates_fail_closed() {
        let mut state = SettlementState::new(AuctionId::parse("auction-1").unwrap());
        assert_eq!(
            state.apply_signal(candidate(1, 1, 500)).0,
            SettlementDisposition::Applied
        );
        assert_eq!(
            state.apply_signal(candidate(1, 1, 500)).0,
            SettlementDisposition::Duplicate
        );
        let mut changed = candidate(1, 1, 600);
        changed.fingerprint = "changed".to_owned();
        assert_eq!(
            state.apply_signal(changed).0,
            SettlementDisposition::ChangedDuplicate
        );

        let mut state = SettlementState::new(AuctionId::parse("auction-1").unwrap());
        state.apply_signal(candidate(2, 2, 700));
        assert_eq!(
            state.apply_signal(candidate(1, 3, 500)).0,
            SettlementDisposition::WrongRound
        );
        assert_eq!(state.current_amount().unwrap().minor_units, 700);
    }

    #[test]
    fn processing_waits_and_success_confirms_once() {
        let mut processing = state_with_attempt(SettlementAttemptStatus::Processing, 1_000);
        assert_eq!(processing.phase, SettlementPhase::WaitingForPayment);
        let action = processing
            .bind_attempt(
                SettlementAttemptStatus::Succeeded {
                    payment_id: "payment-1".to_owned(),
                },
                PaymentAttemptId::parse("attempt-1").unwrap(),
                CandidateId::parse("candidate-1").unwrap(),
                2,
                2_000,
            )
            .unwrap();
        assert!(matches!(action, SettlementAction::ConfirmWinner { .. }));
        let duplicate = processing
            .bind_attempt(
                SettlementAttemptStatus::Succeeded {
                    payment_id: "payment-1".to_owned(),
                },
                PaymentAttemptId::parse("attempt-1").unwrap(),
                CandidateId::parse("candidate-1").unwrap(),
                2,
                2_000,
            )
            .unwrap();
        assert_eq!(duplicate, SettlementAction::None);
    }

    #[test]
    fn recovery_race_uses_authoritative_status_before_expiry_or_forfeit() {
        let start = 10_000;
        let mut recovery = state_with_attempt(SettlementAttemptStatus::RecoveryRequired, start);
        assert_eq!(
            recovery.query().recovery_deadline_millis,
            Some(start + RECOVERY_WINDOW_MILLIS)
        );
        assert_eq!(
            recovery.recovery_timer_fired(start + RECOVERY_WINDOW_MILLIS - 1),
            SettlementAction::None
        );
        assert!(matches!(
            recovery.recovery_timer_fired(start + RECOVERY_WINDOW_MILLIS),
            SettlementAction::ReadAuthoritativeAttempt { .. }
        ));
        assert!(matches!(
            recovery.authoritative_after_recovery_timeout(),
            SettlementAction::ExpirePayment { .. }
        ));

        let mut succeeded = state_with_attempt(SettlementAttemptStatus::RecoveryRequired, start);
        succeeded
            .bind_attempt(
                SettlementAttemptStatus::Succeeded {
                    payment_id: "payment-race".to_owned(),
                },
                PaymentAttemptId::parse("attempt-1").unwrap(),
                CandidateId::parse("candidate-1").unwrap(),
                2,
                start + RECOVERY_WINDOW_MILLIS,
            )
            .unwrap();
        assert!(matches!(
            succeeded.authoritative_after_recovery_timeout(),
            SettlementAction::ConfirmWinner { .. }
        ));
    }

    #[test]
    fn expired_forfeits_then_only_owner_candidate_starts_next_round() {
        let mut state = state_with_attempt(SettlementAttemptStatus::Expired, 1_000);
        assert_eq!(state.phase, SettlementPhase::ForfeitCandidate);
        state.forfeit_confirmed();
        assert_eq!(state.phase, SettlementPhase::AwaitingNextRound);
        let (_, action) = state.apply_signal(candidate(2, 2, 900));
        assert_eq!(
            action,
            SettlementAction::AttemptPayment {
                round: SettlementRound::new(2).unwrap()
            }
        );
        assert_eq!(state.current_amount().unwrap().minor_units, 900);
    }

    #[test]
    fn notification_failure_does_not_change_payment_or_recovery_deadline() {
        let mut state = state_with_attempt(SettlementAttemptStatus::RecoveryRequired, 1_000);
        let before = state.query();
        state.record_notification_failure();
        let after = state.query();
        assert_eq!(after.phase, before.phase);
        assert_eq!(
            after.recovery_deadline_millis,
            before.recovery_deadline_millis
        );
        assert_eq!(after.notification_failures, 1);
    }
}
