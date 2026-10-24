//! Typed Temporal signal-with-start adapter for decoded owner events.

use async_trait::async_trait;
use temporalio_client::{Client, WorkflowIdConflictPolicy, WorkflowStartOptions};

use crate::{
    ingress::{
        DecodedOwnerEvent, LIFECYCLE_WORKFLOW_TYPE, OwnerEvent, SETTLEMENT_WORKFLOW_TYPE,
        SignalReceipt, SignalWithStartClient, SignalWithStartRequest, TemporalGatewayError,
    },
    workflows::{
        AuctionLifecycleInput, AuctionLifecycleWorkflow, CorrelationContext, LifecycleEventKind,
        LifecycleSignal, LifecycleTerminal, ProcessId, SettleAuctionInput, SettleAuctionWorkflow,
        SettlementOwnerSignal, SettlementSignal, SettlementTerminal,
    },
};

#[async_trait]
impl SignalWithStartClient for Client {
    async fn signal_with_start(
        &self,
        request: SignalWithStartRequest,
    ) -> Result<SignalReceipt, TemporalGatewayError> {
        let workflow_id = request.workflow_id.clone();
        let options = WorkflowStartOptions::new(request.task_queue, workflow_id.clone())
            .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
            .build();
        match request.workflow_type {
            LIFECYCLE_WORKFLOW_TYPE => {
                let (input, signal) = lifecycle_payload(request.event, &workflow_id)?;
                self.signal_with_start_workflow(
                    AuctionLifecycleWorkflow::run,
                    input,
                    AuctionLifecycleWorkflow::owner_event,
                    signal,
                    options,
                )
                .await
                .map_err(|_| TemporalGatewayError::Unavailable)?;
            }
            SETTLEMENT_WORKFLOW_TYPE => {
                let (input, signal) = settlement_payload(request.event)?;
                self.signal_with_start_workflow(
                    SettleAuctionWorkflow::run,
                    input,
                    SettleAuctionWorkflow::owner_event,
                    signal,
                    options,
                )
                .await
                .map_err(|_| TemporalGatewayError::Unavailable)?;
            }
            _ => return Err(TemporalGatewayError::Rejected),
        }
        Ok(SignalReceipt {
            workflow_id,
            signal_accepted: true,
        })
    }
}

fn lifecycle_payload(
    event: DecodedOwnerEvent,
    workflow_id: &str,
) -> Result<(AuctionLifecycleInput, LifecycleSignal), TemporalGatewayError> {
    let correlation = correlation(&event);
    let (auction_id, close_at_millis, opened_version, kind) = match event.event {
        OwnerEvent::AuctionObserved { .. } => return Err(TemporalGatewayError::Rejected),
        OwnerEvent::AuctionOpened {
            auction_id,
            ends_at_millis,
        } => (
            auction_id,
            ends_at_millis,
            event.aggregate_version,
            LifecycleEventKind::AuctionOpened {
                close_at_millis: ends_at_millis,
            },
        ),
        OwnerEvent::AuctionDeadlineChanged {
            auction_id,
            ends_at_millis,
        } => (
            auction_id,
            ends_at_millis,
            0,
            LifecycleEventKind::DeadlineExtended {
                close_at_millis: ends_at_millis,
            },
        ),
        OwnerEvent::AuctionClosed { auction_id } => (
            auction_id,
            event.occurred_at_millis,
            0,
            LifecycleEventKind::AuctionClosed,
        ),
        OwnerEvent::WinnerConfirmed { auction_id } => (
            auction_id,
            event.occurred_at_millis,
            0,
            LifecycleEventKind::Terminal {
                outcome: LifecycleTerminal::Sold,
            },
        ),
        OwnerEvent::AuctionUnsold { auction_id } => (
            auction_id,
            event.occurred_at_millis,
            0,
            LifecycleEventKind::Terminal {
                outcome: LifecycleTerminal::Unsold,
            },
        ),
        OwnerEvent::AuctionCancelled { auction_id } => (
            auction_id,
            event.occurred_at_millis,
            0,
            LifecycleEventKind::Terminal {
                outcome: LifecycleTerminal::Cancelled,
            },
        ),
        _ => return Err(TemporalGatewayError::Rejected),
    };
    let signal = LifecycleSignal {
        event_id: event.event_id.clone(),
        fingerprint: event.event_id,
        aggregate_version: event.aggregate_version,
        kind,
    };
    let input = AuctionLifecycleInput {
        process_id: ProcessId::parse(workflow_id).map_err(|_| TemporalGatewayError::Rejected)?,
        auction_id,
        opened_version,
        close_at_millis,
        correlation,
    };
    Ok((input, signal))
}

fn settlement_payload(
    event: DecodedOwnerEvent,
) -> Result<(SettleAuctionInput, SettlementSignal), TemporalGatewayError> {
    let correlation = correlation(&event);
    let (auction_id, kind) = match event.event {
        OwnerEvent::AuctionObserved { .. } => return Err(TemporalGatewayError::Rejected),
        OwnerEvent::SettlementCandidateSelected {
            auction_id,
            candidate_id,
            round,
            amount,
        } => {
            let candidate = crate::workflows::SettlementCandidate {
                auction_id: auction_id.clone(),
                candidate_id,
                round,
                amount,
                auction_version: event.aggregate_version,
            };
            (
                auction_id,
                SettlementOwnerSignal::CandidateSelected { candidate },
            )
        }
        OwnerEvent::PaymentAttemptStarted {
            auction_id,
            attempt_id,
            round,
            ..
        }
        | OwnerEvent::PaymentRecoveryRequired {
            auction_id,
            attempt_id,
            round,
        }
        | OwnerEvent::PaymentSucceeded {
            auction_id,
            attempt_id,
            round,
        }
        | OwnerEvent::PaymentExpired {
            auction_id,
            attempt_id,
            round,
        } => (
            auction_id,
            SettlementOwnerSignal::PaymentChanged {
                attempt_id,
                round,
                attempt_version: event.aggregate_version,
            },
        ),
        OwnerEvent::WinnerConfirmed { auction_id } => (
            auction_id,
            SettlementOwnerSignal::AuctionTerminal {
                outcome: SettlementTerminal::Sold,
            },
        ),
        OwnerEvent::AuctionUnsold { auction_id } => (
            auction_id,
            SettlementOwnerSignal::AuctionTerminal {
                outcome: SettlementTerminal::Unsold,
            },
        ),
        OwnerEvent::AuctionCancelled { auction_id } => (
            auction_id,
            SettlementOwnerSignal::AuctionTerminal {
                outcome: SettlementTerminal::Cancelled,
            },
        ),
        _ => return Err(TemporalGatewayError::Rejected),
    };
    let signal = SettlementSignal {
        event_id: event.event_id.clone(),
        fingerprint: event.event_id,
        owner_version: event.aggregate_version,
        kind,
    };
    Ok((
        SettleAuctionInput {
            auction_id,
            correlation,
        },
        signal,
    ))
}

fn correlation(event: &DecodedOwnerEvent) -> CorrelationContext {
    CorrelationContext {
        correlation_id: event.correlation_id.clone(),
        causation_id: event
            .causation_id
            .clone()
            .unwrap_or_else(|| event.event_id.clone()),
    }
}
