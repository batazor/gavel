use async_trait::async_trait;

use super::{DecodedOwnerEvent, OwnerEvent};
use crate::workflows::ProcessError;

pub const LIFECYCLE_WORKFLOW_TYPE: &str = "AuctionLifecycle";
pub const SETTLEMENT_WORKFLOW_TYPE: &str = "SettleAuction";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowConflictPolicy {
    UseExisting,
}

impl From<WorkflowConflictPolicy> for temporalio_client::WorkflowIdConflictPolicy {
    fn from(value: WorkflowConflictPolicy) -> Self {
        match value {
            WorkflowConflictPolicy::UseExisting => Self::UseExisting,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalWithStartRequest {
    pub workflow_id: String,
    pub workflow_type: &'static str,
    pub task_queue: String,
    pub signal_name: &'static str,
    pub request_id: String,
    pub conflict_policy: WorkflowConflictPolicy,
    pub event: DecodedOwnerEvent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalReceipt {
    pub workflow_id: String,
    pub signal_accepted: bool,
}

#[async_trait]
pub trait SignalWithStartClient: Send + Sync {
    async fn signal_with_start(
        &self,
        request: SignalWithStartRequest,
    ) -> Result<SignalReceipt, TemporalGatewayError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TemporalGatewayError {
    #[error("Temporal is unavailable")]
    Unavailable,
    #[error("Temporal rejected the signal-with-start request")]
    Rejected,
}

#[derive(Clone)]
pub struct TemporalGateway<C> {
    client: C,
    lifecycle_task_queue: String,
    settlement_task_queue: String,
}

impl<C> TemporalGateway<C>
where
    C: SignalWithStartClient,
{
    pub fn new(
        client: C,
        lifecycle_task_queue: impl Into<String>,
        settlement_task_queue: impl Into<String>,
    ) -> Self {
        Self {
            client,
            lifecycle_task_queue: lifecycle_task_queue.into(),
            settlement_task_queue: settlement_task_queue.into(),
        }
    }

    pub async fn signal(&self, event: DecodedOwnerEvent) -> Result<SignalReceipt, ProcessError> {
        if matches!(&event.event, OwnerEvent::AuctionObserved { .. }) {
            return Err(ProcessError::InvalidInput);
        }
        if matches!(
            &event.event,
            OwnerEvent::AuctionCancelled { .. }
                | OwnerEvent::AuctionUnsold { .. }
                | OwnerEvent::WinnerConfirmed { .. }
        ) {
            let lifecycle = self.request(
                event.clone(),
                Route {
                    workflow_id_prefix: "auction-lifecycle",
                    workflow_type: LIFECYCLE_WORKFLOW_TYPE,
                    signal_name: "owner_event",
                },
                Some("lifecycle"),
            )?;
            self.client
                .signal_with_start(lifecycle)
                .await
                .map_err(map_gateway_error)?;
            let settlement = self.request(event, route_for_settlement(), Some("settlement"))?;
            return self
                .client
                .signal_with_start(settlement)
                .await
                .map_err(map_gateway_error);
        }
        let route = route(&event.event);
        let request = self.request(event, route, None)?;
        self.client
            .signal_with_start(request)
            .await
            .map_err(map_gateway_error)
    }

    fn request(
        &self,
        event: DecodedOwnerEvent,
        route: Route,
        request_suffix: Option<&str>,
    ) -> Result<SignalWithStartRequest, ProcessError> {
        let auction_id = auction_id(&event.event);
        Ok(SignalWithStartRequest {
            workflow_id: format!("{}/{auction_id}", route.workflow_id_prefix),
            workflow_type: route.workflow_type,
            task_queue: match route.workflow_type {
                LIFECYCLE_WORKFLOW_TYPE => self.lifecycle_task_queue.clone(),
                SETTLEMENT_WORKFLOW_TYPE => self.settlement_task_queue.clone(),
                _ => return Err(ProcessError::InvariantViolation),
            },
            signal_name: route.signal_name,
            request_id: match request_suffix {
                Some(suffix) => format!("owner-event/{}/{suffix}", event.event_id),
                None => format!("owner-event/{}", event.event_id),
            },
            conflict_policy: WorkflowConflictPolicy::UseExisting,
            event,
        })
    }
}

fn map_gateway_error(error: TemporalGatewayError) -> ProcessError {
    match error {
        TemporalGatewayError::Unavailable => ProcessError::DependencyUnavailable,
        TemporalGatewayError::Rejected => ProcessError::InvariantViolation,
    }
}

struct Route {
    workflow_id_prefix: &'static str,
    workflow_type: &'static str,
    signal_name: &'static str,
}

fn route(event: &OwnerEvent) -> Route {
    match event {
        OwnerEvent::AuctionObserved { .. } => {
            unreachable!("observation events are acknowledged before routing")
        }
        OwnerEvent::AuctionOpened { .. }
        | OwnerEvent::AuctionDeadlineChanged { .. }
        | OwnerEvent::AuctionClosed { .. } => Route {
            workflow_id_prefix: "auction-lifecycle",
            workflow_type: LIFECYCLE_WORKFLOW_TYPE,
            signal_name: "owner_event",
        },
        OwnerEvent::AuctionCancelled { .. }
        | OwnerEvent::AuctionUnsold { .. }
        | OwnerEvent::WinnerConfirmed { .. }
        | OwnerEvent::SettlementCandidateSelected { .. }
        | OwnerEvent::PaymentAttemptStarted { .. }
        | OwnerEvent::PaymentRecoveryRequired { .. }
        | OwnerEvent::PaymentSucceeded { .. }
        | OwnerEvent::PaymentExpired { .. } => route_for_settlement(),
    }
}

const fn route_for_settlement() -> Route {
    Route {
        workflow_id_prefix: "settle-auction",
        workflow_type: SETTLEMENT_WORKFLOW_TYPE,
        signal_name: "owner_event",
    }
}

fn auction_id(event: &OwnerEvent) -> &str {
    match event {
        OwnerEvent::AuctionObserved { auction_id }
        | OwnerEvent::AuctionOpened { auction_id, .. }
        | OwnerEvent::AuctionDeadlineChanged { auction_id, .. }
        | OwnerEvent::AuctionClosed { auction_id }
        | OwnerEvent::AuctionCancelled { auction_id }
        | OwnerEvent::AuctionUnsold { auction_id }
        | OwnerEvent::WinnerConfirmed { auction_id }
        | OwnerEvent::SettlementCandidateSelected { auction_id, .. }
        | OwnerEvent::PaymentAttemptStarted { auction_id, .. }
        | OwnerEvent::PaymentRecoveryRequired { auction_id, .. }
        | OwnerEvent::PaymentSucceeded { auction_id, .. }
        | OwnerEvent::PaymentExpired { auction_id, .. } => auction_id.as_str(),
    }
}
