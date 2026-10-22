#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    Unauthenticated,
    Forbidden,
    NotFound,
    Conflict,
    IdempotencyConflict,
    CommandInProgress,
    DependencyUnavailable,
    Internal,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ApplicationError {
    #[error("{safe_message}")]
    InvalidArgument {
        reason: &'static str,
        safe_message: String,
    },
    #[error("authentication is required")]
    Unauthenticated,
    #[error("operation is not permitted")]
    Forbidden,
    #[error("resource was not found")]
    NotFound,
    #[error("{safe_message}")]
    Conflict {
        reason: &'static str,
        safe_message: String,
    },
    #[error("idempotency key was already used for another request")]
    IdempotencyConflict,
    #[error("command is already in progress")]
    CommandInProgress,
    #[error("required dependency is unavailable")]
    DependencyUnavailable { dependency: &'static str },
    #[error("internal catalog failure")]
    Internal,
}

impl ApplicationError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::Unauthenticated => ErrorCode::Unauthenticated,
            Self::Forbidden => ErrorCode::Forbidden,
            Self::NotFound => ErrorCode::NotFound,
            Self::Conflict { .. } => ErrorCode::Conflict,
            Self::IdempotencyConflict => ErrorCode::IdempotencyConflict,
            Self::CommandInProgress => ErrorCode::CommandInProgress,
            Self::DependencyUnavailable { .. } => ErrorCode::DependencyUnavailable,
            Self::Internal => ErrorCode::Internal,
        }
    }

    pub const fn reason(&self) -> &'static str {
        match self {
            Self::InvalidArgument { reason, .. } | Self::Conflict { reason, .. } => reason,
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::IdempotencyConflict => "idempotency_conflict",
            Self::CommandInProgress => "command_in_progress",
            Self::DependencyUnavailable { .. } => "dependency_unavailable",
            Self::Internal => "internal",
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
    fn taxonomy_is_stable_and_dependency_details_are_private() {
        let cases = [
            (
                ApplicationError::InvalidArgument {
                    reason: "invalid_lot",
                    safe_message: "lot is invalid".to_owned(),
                },
                ErrorCode::InvalidArgument,
                "invalid_lot",
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
                ApplicationError::NotFound,
                ErrorCode::NotFound,
                "NOT_FOUND",
                false,
            ),
            (
                ApplicationError::Conflict {
                    reason: "description_frozen",
                    safe_message: "lot description is frozen".to_owned(),
                },
                ErrorCode::Conflict,
                "description_frozen",
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
                "command_in_progress",
                true,
            ),
            (
                ApplicationError::DependencyUnavailable {
                    dependency: "postgres.secret.internal",
                },
                ErrorCode::DependencyUnavailable,
                "dependency_unavailable",
                true,
            ),
            (
                ApplicationError::Internal,
                ErrorCode::Internal,
                "internal",
                false,
            ),
        ];
        for (error, code, reason, retryable) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.reason(), reason);
            assert_eq!(error.retryable(), retryable);
            assert!(!error.to_string().contains("secret.internal"));
        }
    }
}
