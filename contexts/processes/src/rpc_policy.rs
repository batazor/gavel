use std::time::Duration;

use tonic::Code;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RpcPolicy {
    pub deadline: Duration,
    pub max_attempts: u8,
}

impl Default for RpcPolicy {
    fn default() -> Self {
        Self {
            deadline: Duration::from_secs(3),
            max_attempts: 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    Retryable,
    ExpectedRejection,
    Fatal,
}

pub const fn classify(code: Code) -> FailureClass {
    match code {
        Code::Unavailable | Code::ResourceExhausted | Code::DeadlineExceeded => {
            FailureClass::Retryable
        }
        Code::InvalidArgument
        | Code::FailedPrecondition
        | Code::NotFound
        | Code::PermissionDenied => FailureClass::ExpectedRejection,
        _ => FailureClass::Fatal,
    }
}

/// Stable across Temporal run retries. The run id is intentionally excluded.
pub fn idempotency_key(workflow_id: &str, operation: &str, round: Option<u32>) -> String {
    match round {
        Some(round) => format!("{workflow_id}:{operation}:{round}"),
        None => format!("{workflow_id}:{operation}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_transient_transport_failures() {
        assert_eq!(classify(Code::Unavailable), FailureClass::Retryable);
        assert_eq!(classify(Code::DeadlineExceeded), FailureClass::Retryable);
        assert_eq!(
            classify(Code::FailedPrecondition),
            FailureClass::ExpectedRejection
        );
        assert_eq!(classify(Code::Internal), FailureClass::Fatal);
    }

    #[test]
    fn idempotency_key_is_run_independent_and_round_aware() {
        assert_eq!(
            idempotency_key("settle/auction-1", "attempt-payment", Some(2)),
            "settle/auction-1:attempt-payment:2"
        );
        assert_eq!(
            idempotency_key("lifecycle/auction-1", "close", None),
            "lifecycle/auction-1:close"
        );
    }
}
