//! Closed application error taxonomy.

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    BatchTooLarge,
    Unauthenticated,
    Forbidden,
    IdempotencyConflict,
    CommandInProgress,
    DependencyUnavailable,
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::BatchTooLarge => "BATCH_TOO_LARGE",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Forbidden => "FORBIDDEN",
            Self::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
            Self::CommandInProgress => "COMMAND_IN_PROGRESS",
            Self::DependencyUnavailable => "DEPENDENCY_UNAVAILABLE",
            Self::Internal => "INTERNAL",
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ApplicationError {
    #[error("{safe_message}")]
    InvalidArgument {
        reason: &'static str,
        safe_message: String,
    },
    #[error("request batch exceeds the supported limit")]
    BatchTooLarge,
    #[error("authentication is required")]
    Unauthenticated,
    #[error("permission denied")]
    Forbidden,
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("command is already in progress; retry later")]
    CommandInProgress,
    #[error("required dependency is unavailable")]
    DependencyUnavailable { dependency: &'static str },
    #[error("internal authorization failure")]
    Internal,
}

impl ApplicationError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::BatchTooLarge => ErrorCode::BatchTooLarge,
            Self::Unauthenticated => ErrorCode::Unauthenticated,
            Self::Forbidden => ErrorCode::Forbidden,
            Self::IdempotencyConflict => ErrorCode::IdempotencyConflict,
            Self::CommandInProgress => ErrorCode::CommandInProgress,
            Self::DependencyUnavailable { .. } => ErrorCode::DependencyUnavailable,
            Self::Internal => ErrorCode::Internal,
        }
    }

    pub const fn reason(&self) -> &'static str {
        match self {
            Self::InvalidArgument { reason, .. } => reason,
            Self::IdempotencyConflict => "idempotency_conflict",
            _ => self.code().as_str(),
        }
    }

    pub const fn retryable(&self) -> bool {
        matches!(
            self,
            Self::CommandInProgress | Self::DependencyUnavailable { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taxonomy_has_stable_codes_reasons_and_retryability() {
        let cases = [
            (
                ApplicationError::InvalidArgument {
                    reason: "invalid_resource",
                    safe_message: "resource is invalid".to_owned(),
                },
                ErrorCode::InvalidArgument,
                "invalid_resource",
                false,
            ),
            (
                ApplicationError::BatchTooLarge,
                ErrorCode::BatchTooLarge,
                "BATCH_TOO_LARGE",
                false,
            ),
            (
                ApplicationError::Unauthenticated,
                ErrorCode::Unauthenticated,
                "UNAUTHENTICATED",
                false,
            ),
            (
                ApplicationError::Forbidden,
                ErrorCode::Forbidden,
                "FORBIDDEN",
                false,
            ),
            (
                ApplicationError::IdempotencyConflict,
                ErrorCode::IdempotencyConflict,
                "idempotency_conflict",
                false,
            ),
            (
                ApplicationError::CommandInProgress,
                ErrorCode::CommandInProgress,
                "COMMAND_IN_PROGRESS",
                true,
            ),
            (
                ApplicationError::DependencyUnavailable {
                    dependency: "policy_engine",
                },
                ErrorCode::DependencyUnavailable,
                "DEPENDENCY_UNAVAILABLE",
                true,
            ),
            (
                ApplicationError::Internal,
                ErrorCode::Internal,
                "INTERNAL",
                false,
            ),
        ];

        for (error, expected_code, expected_reason, expected_retryable) in cases {
            assert_eq!(error.code(), expected_code);
            assert_eq!(error.reason(), expected_reason);
            assert_eq!(error.retryable(), expected_retryable);
        }
    }
}
