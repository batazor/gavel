//! Bounded-cardinality telemetry for process workers and workflow completion.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use futures::future::LocalBoxFuture;
use sha2::{Digest, Sha256};
use temporalio_common::protos::coresdk::{
    workflow_activation::{WorkflowActivation, workflow_activation_job},
    workflow_completion::WorkflowActivationCompletion,
};
use temporalio_sdk::{
    WorkerRunError,
    interceptors::{Next, RunWorkerInput, WorkerInterceptor},
};

use crate::bootstrap::RuntimeHealth;

/// Telemetry labels are deliberately static enums expressed as `&'static str`.
/// Workflow, auction, candidate, payment and request identifiers never become labels.
pub trait ProcessTelemetry: Send + Sync {
    fn signal(&self, event_type: &'static str, disposition: &'static str);
    fn activity(&self, activity_type: &'static str, outcome: &'static str);
    fn terminal(&self, workflow_type: &'static str, outcome: &'static str);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopTelemetry;

impl ProcessTelemetry for NoopTelemetry {
    fn signal(&self, _event_type: &'static str, _disposition: &'static str) {}
    fn activity(&self, _activity_type: &'static str, _outcome: &'static str) {}
    fn terminal(&self, _workflow_type: &'static str, _outcome: &'static str) {}
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MetricsTelemetry;

impl ProcessTelemetry for MetricsTelemetry {
    fn signal(&self, event_type: &'static str, disposition: &'static str) {
        metrics::counter!(
            "gavel_process_signal_total",
            "event_type" => bounded_signal_type(event_type),
            "disposition" => bounded_signal_disposition(disposition)
        )
        .increment(1);
    }

    fn activity(&self, activity_type: &'static str, outcome: &'static str) {
        metrics::counter!(
            "gavel_process_activity_total",
            "activity_type" => bounded_activity_type(activity_type),
            "outcome" => bounded_activity_outcome(outcome)
        )
        .increment(1);
    }

    fn terminal(&self, workflow_type: &'static str, outcome: &'static str) {
        metrics::counter!(
            "gavel_process_workflow_terminal_total",
            "workflow_type" => bounded_workflow_type(workflow_type),
            "outcome" => bounded_terminal_outcome(outcome)
        )
        .increment(1);
    }
}

/// Ensures terminal telemetry is emitted once even if a completion is redelivered.
/// Execution identifiers remain process-local dedupe data and are never exported.
pub struct CompletionObserver<T> {
    telemetry: T,
    completed: Mutex<HashSet<[u8; 32]>>,
}

impl<T: ProcessTelemetry> CompletionObserver<T> {
    pub fn new(telemetry: T) -> Self {
        Self {
            telemetry,
            completed: Mutex::new(HashSet::new()),
        }
    }

    pub fn observe(
        &self,
        execution_id: &str,
        workflow_type: &'static str,
        outcome: &'static str,
    ) -> bool {
        let mut completed = self
            .completed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let execution_fingerprint: [u8; 32] = Sha256::digest(execution_id.as_bytes()).into();
        if !completed.insert(execution_fingerprint) {
            return false;
        }
        drop(completed);
        self.telemetry.terminal(workflow_type, outcome);
        true
    }
}

fn bounded_signal_type(value: &'static str) -> &'static str {
    match value {
        "decode"
        | "auction_observed"
        | "auction_opened"
        | "auction_deadline_changed"
        | "auction_closed"
        | "auction_cancelled"
        | "auction_unsold"
        | "winner_confirmed"
        | "candidate_selected"
        | "bidder_outbid"
        | "payment_attempt_started"
        | "payment_recovery_required"
        | "payment_succeeded"
        | "payment_expired" => value,
        _ => "unknown",
    }
}

fn bounded_signal_disposition(value: &'static str) -> &'static str {
    match value {
        "invalid" | "duplicate" | "stale" | "ignored" | "quarantined" | "applied" | "gap" => value,
        _ => "unknown",
    }
}

fn bounded_activity_type(value: &'static str) -> &'static str {
    match value {
        "attempt_payment"
        | "get_settlement_attempt"
        | "expire_payment"
        | "forfeit_candidate"
        | "confirm_winner"
        | "notify"
        | "notify_recovery"
        | "notify_outbid"
        | "close_auction" => value,
        _ => "unknown",
    }
}

fn bounded_activity_outcome(value: &'static str) -> &'static str {
    match value {
        "success" | "retryable_failure" | "owner_rejected" | "invalid_input"
        | "invariant_failure" => value,
        _ => "unknown",
    }
}

fn bounded_workflow_type(value: &'static str) -> &'static str {
    match value {
        "auction_lifecycle" | "settle_auction" | "notify_outbid" => value,
        _ => "unknown",
    }
}

fn bounded_terminal_outcome(value: &'static str) -> &'static str {
    match value {
        "success" | "unexpected_failure" | "cancelled" => value,
        _ => "unknown",
    }
}

/// A safe diagnostic event. Its enum fields cannot carry identifiers, amounts, keys or URLs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessDiagnostic {
    pub component: DiagnosticComponent,
    pub operation: DiagnosticOperation,
    pub outcome: DiagnosticOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticComponent {
    Runtime,
    Ingress,
    Workflow,
    Activity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticOperation {
    Startup,
    Poll,
    Signal,
    OwnerCall,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticOutcome {
    Started,
    Success,
    RetryableFailure,
    Rejected,
    Stopped,
}

impl ProcessDiagnostic {
    pub fn record(self) {
        tracing::info!(
            component = ?self.component,
            operation = ?self.operation,
            outcome = ?self.outcome,
            "process diagnostic"
        );
    }
}

/// Tracks actual worker run-loop lifetime. Combined with the Temporal health probe this keeps
/// readiness false before both pollers start and immediately removes it on any worker exit.
pub struct WorkerReadiness {
    health: Arc<RuntimeHealth>,
}

impl WorkerReadiness {
    pub fn new(health: Arc<RuntimeHealth>) -> Self {
        Self { health }
    }
}

#[async_trait::async_trait(?Send)]
impl WorkerInterceptor for WorkerReadiness {
    fn run_worker<'a>(
        &'a self,
        input: RunWorkerInput<'a>,
        next: Next<'a, RunWorkerInput<'a>, LocalBoxFuture<'a, Result<(), WorkerRunError>>>,
    ) -> LocalBoxFuture<'a, Result<(), WorkerRunError>> {
        Box::pin(async move {
            let _active = self.health.worker_started();
            next.run(input).await
        })
    }
}

pub struct WorkflowCompletionMetrics {
    fallback_workflow_type: &'static str,
    workflow_types: Mutex<HashMap<String, &'static str>>,
    observer: CompletionObserver<MetricsTelemetry>,
}

impl WorkflowCompletionMetrics {
    pub fn new(workflow_type: &'static str) -> Self {
        Self {
            fallback_workflow_type: workflow_type,
            workflow_types: Mutex::new(HashMap::new()),
            observer: CompletionObserver::new(MetricsTelemetry),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl WorkerInterceptor for WorkflowCompletionMetrics {
    async fn on_workflow_activation(
        &self,
        activation: &WorkflowActivation,
    ) -> Result<(), anyhow::Error> {
        let workflow_type = activation.jobs.iter().find_map(|job| match &job.variant {
            Some(workflow_activation_job::Variant::InitializeWorkflow(initialization)) => {
                Some(workflow_metric_type(&initialization.workflow_type))
            }
            _ => None,
        });
        if let Some(workflow_type) = workflow_type {
            self.workflow_types
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(activation.run_id.clone(), workflow_type);
        }
        Ok(())
    }

    async fn on_workflow_activation_completion(&self, completion: &WorkflowActivationCompletion) {
        let outcome = if completion.has_complete_workflow_execution() {
            Some("success")
        } else if completion.has_fail_execution() {
            Some("unexpected_failure")
        } else if completion.has_cancel_workflow_execution() {
            Some("cancelled")
        } else {
            None
        };
        if let Some(outcome) = outcome {
            let workflow_type = self
                .workflow_types
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&completion.run_id)
                .unwrap_or(self.fallback_workflow_type);
            self.observer
                .observe(&completion.run_id, workflow_type, outcome);
        }
    }
}

fn workflow_metric_type(workflow_type: &str) -> &'static str {
    match workflow_type {
        "AuctionLifecycleWorkflow" => "auction_lifecycle",
        "SettleAuctionWorkflow" => "settle_auction",
        "NotifyOutbidWorkflow" => "notify_outbid",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Default)]
    struct CountingTelemetry(AtomicUsize);

    impl ProcessTelemetry for CountingTelemetry {
        fn signal(&self, _: &'static str, _: &'static str) {}
        fn activity(&self, _: &'static str, _: &'static str) {}
        fn terminal(&self, _: &'static str, _: &'static str) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn completion_is_observed_once_per_execution() {
        let observer = CompletionObserver::new(CountingTelemetry::default());

        assert!(observer.observe("execution-a", "settlement", "sold"));
        assert!(!observer.observe("execution-a", "settlement", "sold"));
        assert!(observer.observe("execution-b", "settlement", "unsold"));
        assert_eq!(observer.telemetry.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn custom_metric_labels_are_bounded_and_unknown_values_are_redacted() {
        assert_eq!(bounded_signal_type("auction_opened"), "auction_opened");
        assert_eq!(bounded_signal_type("bidder_outbid"), "bidder_outbid");
        assert_eq!(bounded_signal_type("auction/customer-42"), "unknown");
        assert_eq!(bounded_signal_disposition("https://secret"), "unknown");
        assert_eq!(bounded_activity_type("attempt_payment"), "attempt_payment");
        assert_eq!(bounded_activity_type("notify_outbid"), "notify_outbid");
        assert_eq!(bounded_activity_type("idempotency-key-42"), "unknown");
        assert_eq!(bounded_activity_outcome("1000000"), "unknown");
        assert_eq!(bounded_workflow_type("settle_auction"), "settle_auction");
        assert_eq!(
            workflow_metric_type("NotifyOutbidWorkflow"),
            "notify_outbid"
        );
        assert_eq!(workflow_metric_type("customer-workflow-id"), "unknown");
        assert_eq!(bounded_terminal_outcome("provider-reference"), "unknown");
    }

    #[test]
    fn diagnostic_shape_cannot_accept_arbitrary_sensitive_fields() {
        let event = ProcessDiagnostic {
            component: DiagnosticComponent::Runtime,
            operation: DiagnosticOperation::Shutdown,
            outcome: DiagnosticOutcome::Stopped,
        };
        let rendered = format!("{event:?}");
        for forbidden in ["auction_id", "amount", "idempotency", "recovery_url"] {
            assert!(!rendered.contains(forbidden));
        }
    }
}
