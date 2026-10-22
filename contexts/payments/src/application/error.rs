use crate::domain::DomainError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    NotFound,
    PermissionDenied,
    InvalidState,
    IdempotencyConflict,
    InProgress,
    DependencyUnavailable,
    AlreadySucceeded,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApplicationError {
    #[error("invalid input")]
    InvalidInput,
    #[error("resource not found")]
    NotFound,
    #[error("permission denied")]
    PermissionDenied,
    #[error("operation is not valid in the current state")]
    InvalidState,
    #[error("idempotency key was used for a different command")]
    IdempotencyConflict,
    #[error("operation is already in progress")]
    InProgress,
    #[error("dependency is unavailable")]
    DependencyUnavailable,
    #[error("payment has already succeeded")]
    AlreadySucceeded,
    #[error("internal error")]
    Internal,
}

impl ApplicationError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidInput => ErrorCode::InvalidInput,
            Self::NotFound => ErrorCode::NotFound,
            Self::PermissionDenied => ErrorCode::PermissionDenied,
            Self::InvalidState => ErrorCode::InvalidState,
            Self::IdempotencyConflict => ErrorCode::IdempotencyConflict,
            Self::InProgress => ErrorCode::InProgress,
            Self::DependencyUnavailable => ErrorCode::DependencyUnavailable,
            Self::AlreadySucceeded => ErrorCode::AlreadySucceeded,
            Self::Internal => ErrorCode::Internal,
        }
    }
}

impl From<DomainError> for ApplicationError {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::InvalidTransition => Self::InvalidState,
            DomainError::VersionOverflow => Self::Internal,
            _ => Self::InvalidInput,
        }
    }
}
