use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

use crate::application::{
    ApplicationError, ErrorCode, MediaObservation, MediaOperation, MediaOutcome, MediaTelemetry,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    LotQuery,
    LotsQuery,
    RequestUpload,
    CompleteUpload,
    ListLot,
    UpdateLot,
    WithdrawLot,
    Outbox,
    Cleanup,
    GrantReconciliation,
    Dependency,
}

impl Operation {
    const COUNT: usize = 11;
    const fn index(self) -> usize {
        self as usize
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::LotQuery => "lot_query",
            Self::LotsQuery => "lots_query",
            Self::RequestUpload => "request_upload",
            Self::CompleteUpload => "complete_upload",
            Self::ListLot => "list_lot",
            Self::UpdateLot => "update_lot",
            Self::WithdrawLot => "withdraw_lot",
            Self::Outbox => "outbox",
            Self::Cleanup => "cleanup",
            Self::GrantReconciliation => "grant_reconciliation",
            Self::Dependency => "dependency",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Replay,
    Invalid,
    Forbidden,
    Conflict,
    Missing,
    DependencyFailure,
    InternalFailure,
}

impl Outcome {
    const COUNT: usize = 8;
    const fn index(self) -> usize {
        self as usize
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Replay => "replay",
            Self::Invalid => "invalid",
            Self::Forbidden => "forbidden",
            Self::Conflict => "conflict",
            Self::Missing => "missing",
            Self::DependencyFailure => "dependency_failure",
            Self::InternalFailure => "internal_failure",
        }
    }

    pub const fn from_error(error: &ApplicationError) -> Self {
        match error.code() {
            ErrorCode::InvalidArgument | ErrorCode::Unauthenticated => Self::Invalid,
            ErrorCode::Forbidden => Self::Forbidden,
            ErrorCode::NotFound => Self::Missing,
            ErrorCode::Conflict | ErrorCode::IdempotencyConflict | ErrorCode::CommandInProgress => {
                Self::Conflict
            }
            ErrorCode::DependencyUnavailable => Self::DependencyFailure,
            ErrorCode::Internal => Self::InternalFailure,
        }
    }
}

struct Counters {
    calls: Vec<AtomicU64>,
    duration_micros: Vec<AtomicU64>,
    pending_media_age_seconds: AtomicU64,
    outbox_backlog: AtomicU64,
    reconciliation_backlog: AtomicU64,
}

impl Counters {
    fn new() -> Self {
        let size = Operation::COUNT * Outcome::COUNT;
        Self {
            calls: (0..size).map(|_| AtomicU64::new(0)).collect(),
            duration_micros: (0..size).map(|_| AtomicU64::new(0)).collect(),
            pending_media_age_seconds: AtomicU64::new(0),
            outbox_backlog: AtomicU64::new(0),
            reconciliation_backlog: AtomicU64::new(0),
        }
    }
}

fn counters() -> &'static Counters {
    static COUNTERS: OnceLock<Counters> = OnceLock::new();
    COUNTERS.get_or_init(Counters::new)
}

pub fn record(operation: Operation, outcome: Outcome, duration_micros: u64) {
    let index = operation.index() * Outcome::COUNT + outcome.index();
    counters().calls[index].fetch_add(1, Ordering::Relaxed);
    counters().duration_micros[index].fetch_add(duration_micros, Ordering::Relaxed);
    tracing::info!(
        target: "gavel.catalog.operation",
        operation = operation.label(),
        outcome = outcome.label(),
        "catalog operation completed"
    );
}

pub fn render_prometheus() -> String {
    use std::fmt::Write;
    let mut output = String::from(
        "# HELP gavel_catalog_operations_total Bounded Catalog operation outcomes.\n\
         # TYPE gavel_catalog_operations_total counter\n\
         # HELP gavel_catalog_operation_duration_micros_total Aggregate operation duration.\n\
         # TYPE gavel_catalog_operation_duration_micros_total counter\n",
    );
    for operation in [
        Operation::LotQuery,
        Operation::LotsQuery,
        Operation::RequestUpload,
        Operation::CompleteUpload,
        Operation::ListLot,
        Operation::UpdateLot,
        Operation::WithdrawLot,
        Operation::Outbox,
        Operation::Cleanup,
        Operation::GrantReconciliation,
        Operation::Dependency,
    ] {
        for outcome in [
            Outcome::Success,
            Outcome::Replay,
            Outcome::Invalid,
            Outcome::Forbidden,
            Outcome::Conflict,
            Outcome::Missing,
            Outcome::DependencyFailure,
            Outcome::InternalFailure,
        ] {
            let index = operation.index() * Outcome::COUNT + outcome.index();
            writeln!(
                output,
                "gavel_catalog_operations_total{{operation=\"{}\",outcome=\"{}\"}} {}",
                operation.label(),
                outcome.label(),
                counters().calls[index].load(Ordering::Relaxed)
            )
            .expect("writing to String cannot fail");
            writeln!(
                output,
                "gavel_catalog_operation_duration_micros_total{{operation=\"{}\",outcome=\"{}\"}} {}",
                operation.label(), outcome.label(), counters().duration_micros[index].load(Ordering::Relaxed)
            ).expect("writing to String cannot fail");
        }
    }
    writeln!(
        output,
        "# TYPE gavel_catalog_pending_media_oldest_age_seconds gauge"
    )
    .expect("writing to String cannot fail");
    writeln!(
        output,
        "gavel_catalog_pending_media_oldest_age_seconds {}",
        counters().pending_media_age_seconds.load(Ordering::Relaxed)
    )
    .expect("writing to String cannot fail");
    writeln!(output, "# TYPE gavel_catalog_outbox_backlog gauge")
        .expect("writing to String cannot fail");
    writeln!(
        output,
        "gavel_catalog_outbox_backlog {}",
        counters().outbox_backlog.load(Ordering::Relaxed)
    )
    .expect("writing to String cannot fail");
    writeln!(output, "# TYPE gavel_catalog_reconciliation_backlog gauge")
        .expect("writing to String cannot fail");
    writeln!(
        output,
        "gavel_catalog_reconciliation_backlog {}",
        counters().reconciliation_backlog.load(Ordering::Relaxed)
    )
    .expect("writing to String cannot fail");
    output
}

pub fn set_backlogs(pending_media_age_seconds: u64, outbox: u64, reconciliation: u64) {
    counters()
        .pending_media_age_seconds
        .store(pending_media_age_seconds, Ordering::Relaxed);
    counters().outbox_backlog.store(outbox, Ordering::Relaxed);
    counters()
        .reconciliation_backlog
        .store(reconciliation, Ordering::Relaxed);
}

pub fn redact_field(field: &str, value: &str) -> String {
    let field = field.to_ascii_lowercase();
    if [
        "id",
        "key",
        "token",
        "url",
        "checksum",
        "credential",
        "secret",
        "principal",
        "seller",
        "object",
    ]
    .iter()
    .any(|fragment| field.contains(fragment))
    {
        "[REDACTED]".to_owned()
    } else {
        value.to_owned()
    }
}

#[derive(Default)]
pub struct CatalogTelemetry;

impl MediaTelemetry for CatalogTelemetry {
    fn record(&self, observation: MediaObservation) {
        let operation = match observation.operation {
            MediaOperation::RequestUpload => Operation::RequestUpload,
            MediaOperation::CompleteUpload => Operation::CompleteUpload,
            MediaOperation::Cleanup => Operation::Cleanup,
        };
        let outcome = match observation.outcome {
            MediaOutcome::Success => Outcome::Success,
            MediaOutcome::Replay => Outcome::Replay,
            MediaOutcome::Invalid => Outcome::Invalid,
            MediaOutcome::Conflict => Outcome::Conflict,
            MediaOutcome::Missing => Outcome::Missing,
            MediaOutcome::DependencyFailure => Outcome::DependencyFailure,
        };
        record(operation, outcome, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_have_only_bounded_labels_and_redaction_covers_private_values() {
        record(Operation::ListLot, Outcome::Success, 7);
        record(Operation::Outbox, Outcome::DependencyFailure, 11);
        let rendered = render_prometheus();
        assert!(rendered.contains("operation=\"list_lot\",outcome=\"success\""));
        assert!(rendered.contains("operation=\"outbox\",outcome=\"dependency_failure\""));
        for private in [
            "seller-42",
            "image-42",
            "private/key",
            "https://signed",
            "secret-token",
        ] {
            assert!(!rendered.contains(private));
        }
        for field in [
            "seller_id",
            "imageId",
            "object_key",
            "upload_url",
            "checksum",
            "authz_token",
            "credential",
        ] {
            assert_eq!(redact_field(field, "private"), "[REDACTED]");
        }
        assert_eq!(redact_field("outcome", "success"), "success");
    }
}
