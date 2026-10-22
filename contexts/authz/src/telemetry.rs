use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::application::{ApplicationError, ErrorCode, MutationOperation, PermissionDecision};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionOperation {
    Check,
    BulkCheck,
}

impl PermissionOperation {
    const fn index(self) -> usize {
        self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::BulkCheck => "bulk_check",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionOutcome {
    Allowed,
    Denied,
    Mixed,
    Invalid,
    Unauthenticated,
    Forbidden,
    DependencyFailure,
    InternalFailure,
}

impl PermissionOutcome {
    const fn index(self) -> usize {
        self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Mixed => "mixed",
            Self::Invalid => "invalid",
            Self::Unauthenticated => "unauthenticated",
            Self::Forbidden => "forbidden",
            Self::DependencyFailure => "dependency_failure",
            Self::InternalFailure => "internal_failure",
        }
    }

    pub const fn from_error(error: &ApplicationError) -> Self {
        match error.code() {
            ErrorCode::InvalidArgument | ErrorCode::BatchTooLarge => Self::Invalid,
            ErrorCode::Unauthenticated => Self::Unauthenticated,
            ErrorCode::Forbidden => Self::Forbidden,
            ErrorCode::DependencyUnavailable | ErrorCode::CommandInProgress => {
                Self::DependencyFailure
            }
            ErrorCode::IdempotencyConflict | ErrorCode::Internal => Self::InternalFailure,
        }
    }

    pub fn from_decisions(decisions: &[PermissionDecision]) -> Self {
        let has_allowed = decisions.contains(&PermissionDecision::Allowed);
        let has_denied = decisions.contains(&PermissionDecision::Denied);
        match (has_allowed, has_denied) {
            (true, false) => Self::Allowed,
            (false, true) => Self::Denied,
            (true, true) => Self::Mixed,
            (false, false) => Self::InternalFailure,
        }
    }
}

const OUTCOME_COUNT: usize = 8;
const RECEIPT_OUTCOME_COUNT: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptOutcome {
    Claimed,
    Replayed,
    Conflict,
    InProgress,
    Recovered,
    Completed,
    Failed,
}

impl ReceiptOutcome {
    const fn index(self) -> usize {
        self as usize
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::Replayed => "replayed",
            Self::Conflict => "conflict",
            Self::InProgress => "in_progress",
            Self::Recovered => "recovered",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationshipMutationKind {
    Touch,
    Delete,
}

impl RelationshipMutationKind {
    const fn index(self) -> usize {
        self as usize
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Touch => "touch",
            Self::Delete => "delete",
        }
    }
}

struct PermissionTelemetry {
    calls: Vec<AtomicU64>,
    duration_micros: Vec<AtomicU64>,
    receipts: Vec<AtomicU64>,
    relationship_mutations: Vec<AtomicU64>,
}

impl PermissionTelemetry {
    fn new() -> Self {
        let counters = || (0..2 * OUTCOME_COUNT).map(|_| AtomicU64::new(0)).collect();
        Self {
            calls: counters(),
            duration_micros: counters(),
            receipts: (0..2 * RECEIPT_OUTCOME_COUNT)
                .map(|_| AtomicU64::new(0))
                .collect(),
            relationship_mutations: (0..2).map(|_| AtomicU64::new(0)).collect(),
        }
    }
}

pub fn record_receipt(operation: MutationOperation, outcome: ReceiptOutcome) {
    let operation_index = mutation_operation_index(operation);
    telemetry().receipts[operation_index * RECEIPT_OUTCOME_COUNT + outcome.index()]
        .fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        target: "gavel.authz.receipt",
        operation = operation.as_str(),
        outcome = outcome.label(),
        "authorization mutation receipt"
    );
}

pub fn record_relationship_mutations(kind: RelationshipMutationKind, count: usize) {
    telemetry().relationship_mutations[kind.index()].fetch_add(count as u64, Ordering::Relaxed);
    tracing::info!(
        target: "gavel.authz.relationship",
        mutation = kind.label(),
        count,
        "authorization relationship mutation"
    );
}

const fn mutation_operation_index(operation: MutationOperation) -> usize {
    match operation {
        MutationOperation::RegisterPrincipal => 0,
        MutationOperation::WriteRelationships => 1,
    }
}

fn telemetry() -> &'static PermissionTelemetry {
    static VALUE: OnceLock<PermissionTelemetry> = OnceLock::new();
    VALUE.get_or_init(PermissionTelemetry::new)
}

pub fn record_permission(
    operation: PermissionOperation,
    outcome: PermissionOutcome,
    duration: Duration,
) {
    let index = operation.index() * OUTCOME_COUNT + outcome.index();
    telemetry().calls[index].fetch_add(1, Ordering::Relaxed);
    telemetry().duration_micros[index].fetch_add(
        duration.as_micros().min(u128::from(u64::MAX)) as u64,
        Ordering::Relaxed,
    );
    tracing::info!(
        target: "gavel.authz.permission",
        operation = operation.label(),
        outcome = outcome.label(),
        "authorization permission decision"
    );
}

pub fn render_permission_prometheus() -> String {
    let mut output = String::new();
    for operation in [PermissionOperation::Check, PermissionOperation::BulkCheck] {
        for outcome in [
            PermissionOutcome::Allowed,
            PermissionOutcome::Denied,
            PermissionOutcome::Mixed,
            PermissionOutcome::Invalid,
            PermissionOutcome::Unauthenticated,
            PermissionOutcome::Forbidden,
            PermissionOutcome::DependencyFailure,
            PermissionOutcome::InternalFailure,
        ] {
            let index = operation.index() * OUTCOME_COUNT + outcome.index();
            push_metric(
                &mut output,
                "gavel_authz_permission_checks_total",
                operation,
                outcome,
                telemetry().calls[index].load(Ordering::Relaxed),
            );
            push_metric(
                &mut output,
                "gavel_authz_permission_duration_micros_total",
                operation,
                outcome,
                telemetry().duration_micros[index].load(Ordering::Relaxed),
            );
        }
    }
    for operation in [
        MutationOperation::RegisterPrincipal,
        MutationOperation::WriteRelationships,
    ] {
        for outcome in [
            ReceiptOutcome::Claimed,
            ReceiptOutcome::Replayed,
            ReceiptOutcome::Conflict,
            ReceiptOutcome::InProgress,
            ReceiptOutcome::Recovered,
            ReceiptOutcome::Completed,
            ReceiptOutcome::Failed,
        ] {
            use std::fmt::Write;
            writeln!(
                output,
                "gavel_authz_receipts_total{{operation=\"{}\",outcome=\"{}\"}} {}",
                operation.as_str(),
                outcome.label(),
                telemetry().receipts
                    [mutation_operation_index(operation) * RECEIPT_OUTCOME_COUNT + outcome.index()]
                .load(Ordering::Relaxed)
            )
            .expect("writing to a String cannot fail");
        }
    }
    for kind in [
        RelationshipMutationKind::Touch,
        RelationshipMutationKind::Delete,
    ] {
        use std::fmt::Write;
        writeln!(
            output,
            "gavel_authz_relationship_mutations_total{{mutation=\"{}\"}} {}",
            kind.label(),
            telemetry().relationship_mutations[kind.index()].load(Ordering::Relaxed)
        )
        .expect("writing to a String cannot fail");
    }
    output
}

pub fn redact_field(field: &str, value: &str) -> String {
    let normalized = field.to_ascii_lowercase();
    let sensitive = [
        "id",
        "token",
        "key",
        "credential",
        "secret",
        "issuer",
        "subject",
        "resource",
        "relationship",
    ]
    .iter()
    .any(|fragment| normalized.contains(fragment));
    if sensitive {
        "[REDACTED]".to_owned()
    } else {
        value.to_owned()
    }
}

fn push_metric(
    output: &mut String,
    name: &str,
    operation: PermissionOperation,
    outcome: PermissionOutcome,
    value: u64,
) {
    use std::fmt::Write;
    writeln!(
        output,
        "{name}{{operation=\"{}\",outcome=\"{}\"}} {value}",
        operation.label(),
        outcome.label()
    )
    .expect("writing to a String cannot fail");
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };

    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = Buffer;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn permission_telemetry_has_only_bounded_labels_and_safe_logs() {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(buffer.clone())
            .finish();
        with_default(subscriber, || {
            record_permission(
                PermissionOperation::BulkCheck,
                PermissionOutcome::Mixed,
                Duration::from_millis(7),
            );
            record_receipt(
                MutationOperation::WriteRelationships,
                ReceiptOutcome::Recovered,
            );
            record_relationship_mutations(RelationshipMutationKind::Delete, 2);
        });

        let metrics = render_permission_prometheus();
        let logs = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(metrics.contains("gavel_authz_permission_checks_total"));
        assert!(metrics.contains("operation=\"bulk_check\",outcome=\"mixed\""));
        assert!(metrics.contains("gavel_authz_receipts_total"));
        assert!(metrics.contains("gavel_authz_relationship_mutations_total"));
        assert!(logs.contains("bulk_check"));
        assert!(logs.contains("mixed"));
        assert!(logs.contains("recovered"));
        assert!(logs.contains("delete"));
        for outcome in [
            "allowed",
            "denied",
            "mixed",
            "invalid",
            "unauthenticated",
            "forbidden",
            "dependency_failure",
            "internal_failure",
            "claimed",
            "replayed",
            "conflict",
            "in_progress",
            "recovered",
            "completed",
            "failed",
        ] {
            assert!(metrics.contains(&format!("outcome=\"{outcome}\"")));
        }
        for forbidden in [
            "resource_id",
            "principal_id",
            "consistency_token",
            "zed_token",
            "credential",
            "idempotency_key",
        ] {
            assert!(
                !metrics.contains(forbidden),
                "unsafe metric label: {forbidden}"
            );
            assert!(!logs.contains(forbidden), "unsafe log field: {forbidden}");
        }
    }

    #[test]
    fn structured_field_redaction_is_default_deny_for_identity_and_secrets() {
        let sensitive = [
            ("resource_id", "auction-42"),
            ("principalId", "u1_user"),
            ("zed_token", "revision-secret"),
            ("authorization_key", "bearer-secret"),
            ("issuer", "https://identity.example"),
            ("subject", "external-user"),
            ("relationship", "auction:42#participant@user:7"),
        ];
        for (field, value) in sensitive {
            assert_eq!(redact_field(field, value), "[REDACTED]");
        }
        assert_eq!(redact_field("operation", "check"), "check");
        assert_eq!(redact_field("outcome", "allowed"), "allowed");
    }
}
