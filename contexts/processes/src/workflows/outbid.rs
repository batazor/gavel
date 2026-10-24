use serde::{Deserialize, Serialize};
use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{
    ApplicationFailure, SyncWorkflowContext, WorkflowContext, WorkflowContextView, WorkflowResult,
};

use super::NotifyOutbidInput;
use crate::{
    activities::{OutbidNotificationIntent, SettlementTemporalActivities},
    adapters::owner_rpc::temporal_activity_options,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutbidSignal {
    pub event_id: String,
    pub aggregate_version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutbidResult {
    pub notification_id: String,
    pub accepted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutbidQuery {
    pub signal_received: bool,
    pub signal_valid: bool,
}

#[workflow]
pub struct NotifyOutbidWorkflow {
    input: NotifyOutbidInput,
    signal_received: bool,
    signal_valid: bool,
}

#[workflow_methods]
impl NotifyOutbidWorkflow {
    #[init]
    pub fn new(_context: &WorkflowContextView, input: NotifyOutbidInput) -> Self {
        Self {
            input,
            signal_received: false,
            signal_valid: true,
        }
    }

    #[run]
    pub async fn run(context: &mut WorkflowContext<Self>) -> WorkflowResult<OutbidResult> {
        context
            .wait_condition(|workflow| workflow.signal_received)
            .await?;
        let (input, signal_valid) =
            context.state(|workflow| (workflow.input.clone(), workflow.signal_valid));
        if !signal_valid || input.aggregate_version == 0 {
            return Err(ApplicationFailure::non_retryable(
                "outbid signal does not match workflow input",
            )
            .into());
        }
        let acknowledgement = context
            .execute_activity(
                SettlementTemporalActivities::notify_outbid,
                OutbidNotificationIntent {
                    auction_id: input.auction_id,
                    aggregate_version: input.aggregate_version,
                    recipient_id: input.recipient_id,
                    correlation_id: input.correlation.correlation_id,
                },
                temporal_activity_options(),
            )
            .await?;
        Ok(OutbidResult {
            notification_id: acknowledgement.notification_id,
            accepted: acknowledgement.accepted,
        })
    }

    #[signal]
    pub fn owner_event(&mut self, _context: &mut SyncWorkflowContext<Self>, signal: OutbidSignal) {
        self.signal_received = true;
        self.signal_valid = signal.event_id == self.input.event_id
            && signal.aggregate_version == self.input.aggregate_version;
    }

    #[query]
    pub fn outbid_state(&self, _context: &WorkflowContextView) -> OutbidQuery {
        OutbidQuery {
            signal_received: self.signal_received,
            signal_valid: self.signal_valid,
        }
    }
}

pub use notify_outbid_workflow::Run as NotifyOutbidRun;
