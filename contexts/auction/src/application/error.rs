use thiserror::Error;

use crate::domain::AuctionError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    Unauthenticated,
    Forbidden,
    NotFound,
    PaymentMethodRequired,
    IdempotencyConflict,
    Contention,
    DependencyUnavailable,
    DomainRejected,
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::PaymentMethodRequired => "PAYMENT_METHOD_REQUIRED",
            Self::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
            Self::Contention => "CONTENTION",
            Self::DependencyUnavailable => "DEPENDENCY_UNAVAILABLE",
            Self::DomainRejected => "DOMAIN_REJECTED",
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
    #[error("authentication is required")]
    Unauthenticated,
    #[error("permission denied")]
    Forbidden,
    #[error("auction not found")]
    NotFound,
    #[error("a ready payment instrument is required")]
    PaymentMethodRequired,
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("auction is busy; retry the command")]
    Contention,
    #[error("required dependency is unavailable")]
    DependencyUnavailable { dependency: &'static str },
    #[error("{safe_message}")]
    DomainRejected {
        reason: &'static str,
        safe_message: String,
    },
    #[error("internal auction failure")]
    Internal,
}

impl ApplicationError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::Unauthenticated => ErrorCode::Unauthenticated,
            Self::Forbidden => ErrorCode::Forbidden,
            Self::NotFound => ErrorCode::NotFound,
            Self::PaymentMethodRequired => ErrorCode::PaymentMethodRequired,
            Self::IdempotencyConflict => ErrorCode::IdempotencyConflict,
            Self::Contention => ErrorCode::Contention,
            Self::DependencyUnavailable { .. } => ErrorCode::DependencyUnavailable,
            Self::DomainRejected { .. } => ErrorCode::DomainRejected,
            Self::Internal => ErrorCode::Internal,
        }
    }

    pub const fn retryable(&self) -> bool {
        matches!(self, Self::Contention | Self::DependencyUnavailable { .. })
    }

    pub fn reason(&self) -> &'static str {
        match self {
            Self::InvalidArgument { reason, .. } | Self::DomainRejected { reason, .. } => reason,
            _ => self.code().as_str(),
        }
    }
}

impl From<AuctionError> for ApplicationError {
    fn from(error: AuctionError) -> Self {
        let rejected = |reason, safe_message: &str| Self::DomainRejected {
            reason,
            safe_message: safe_message.to_owned(),
        };

        match error {
            AuctionError::NotOpened => Self::NotFound,
            AuctionError::NotSeller => Self::Forbidden,
            AuctionError::PaymentInstrumentRequired => Self::PaymentMethodRequired,
            AuctionError::AlreadyOpened => rejected("already_opened", "auction is already open"),
            AuctionError::NotOpen => rejected("not_open", "auction is not open"),
            AuctionError::NotSettling => rejected("not_settling", "auction is not settling"),
            AuctionError::InvalidBidStep => {
                rejected("invalid_bid_step", "bid step must be positive")
            }
            AuctionError::DeadlineNotFuture => {
                rejected("deadline_not_future", "deadline must be in the future")
            }
            AuctionError::DeadlineNotReached { .. } => {
                rejected("too_early", "auction deadline has not yet been reached")
            }
            AuctionError::BiddingEnded => rejected("bidding_ended", "bidding has ended"),
            AuctionError::CurrencyMismatch => rejected(
                "currency_mismatch",
                "bid currency differs from auction currency",
            ),
            AuctionError::MaximumNotAboveCurrentPrice { .. } => rejected(
                "maximum_not_above_current_price",
                "maximum must exceed the current price",
            ),
            AuctionError::MaximumNotRaised => rejected(
                "maximum_not_raised",
                "a bidder may only raise their own maximum",
            ),
            AuctionError::NoEligibleBids => {
                rejected("no_eligible_bids", "there are no eligible bids")
            }
            AuctionError::NoSettlementCandidate => rejected(
                "no_settlement_candidate",
                "there is no settlement candidate",
            ),
            AuctionError::SettlementCandidateMismatch => rejected(
                "settlement_candidate_mismatch",
                "settlement candidate or round does not match",
            ),
            AuctionError::ClosingPolicy(_) => {
                rejected("invalid_closing_policy", "closing policy is invalid")
            }
            AuctionError::Overflow | AuctionError::CorruptEventStream(_) => Self::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Timestamp;

    #[test]
    fn domain_errors_have_stable_safe_classification() {
        let cases = [
            (AuctionError::NotOpened, ErrorCode::NotFound, "NOT_FOUND"),
            (AuctionError::NotSeller, ErrorCode::Forbidden, "FORBIDDEN"),
            (
                AuctionError::PaymentInstrumentRequired,
                ErrorCode::PaymentMethodRequired,
                "PAYMENT_METHOD_REQUIRED",
            ),
            (
                AuctionError::DeadlineNotReached {
                    ends_at: Timestamp::from_millis(99),
                },
                ErrorCode::DomainRejected,
                "too_early",
            ),
            (AuctionError::Overflow, ErrorCode::Internal, "INTERNAL"),
        ];

        for (source, expected_code, expected_reason) in cases {
            let mapped = ApplicationError::from(source);
            assert_eq!(mapped.code(), expected_code);
            assert_eq!(mapped.reason(), expected_reason);
        }
    }
}
