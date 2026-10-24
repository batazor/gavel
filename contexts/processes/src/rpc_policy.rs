use std::{fmt, time::Duration};

use tonic::Code;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub initial_interval: Duration,
    pub backoff_coefficient: u32,
    pub maximum_interval: Duration,
    pub maximum_attempts: u32,
    pub jitter: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivityPolicy {
    pub start_to_close_timeout: Duration,
    pub retry: RetryPolicy,
}

impl Default for ActivityPolicy {
    fn default() -> Self {
        Self {
            start_to_close_timeout: Duration::from_secs(3),
            retry: RetryPolicy {
                initial_interval: Duration::from_millis(200),
                backoff_coefficient: 2,
                maximum_interval: Duration::from_secs(2),
                maximum_attempts: 4,
                jitter: true,
            },
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
        Code::Ok
        | Code::Cancelled
        | Code::Unknown
        | Code::AlreadyExists
        | Code::Aborted
        | Code::OutOfRange
        | Code::Unimplemented
        | Code::Internal
        | Code::DataLoss
        | Code::Unauthenticated => FailureClass::Fatal,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationTemplate {
    Settlement,
    CandidateFailed,
    Recovery,
    Winner,
}

impl NotificationTemplate {
    const fn key_segment(self) -> &'static str {
        match self {
            Self::Settlement => "settlement",
            Self::CandidateFailed => "candidate-failed",
            Self::Recovery => "recovery",
            Self::Winner => "winner",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    CloseAuction,
    AttemptPayment {
        round: u32,
    },
    ExpirePayment {
        round: u32,
    },
    ForfeitSettlementCandidate {
        round: u32,
    },
    ConfirmWinner {
        round: u32,
    },
    Notify {
        template: NotificationTemplate,
        round: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentKey(String);

impl IntentKey {
    pub fn for_auction(auction_id: &str, intent: Intent) -> Result<Self, IntentKeyError> {
        if !valid_segment(auction_id) {
            return Err(IntentKeyError::InvalidAuctionId);
        }
        let key = match intent {
            Intent::CloseAuction => format!("auction-lifecycle/{auction_id}:close"),
            Intent::AttemptPayment { round } => {
                settlement_key(auction_id, "attempt-payment", validate_round(round)?)
            }
            Intent::ExpirePayment { round } => {
                settlement_key(auction_id, "expire-payment", validate_round(round)?)
            }
            Intent::ForfeitSettlementCandidate { round } => {
                settlement_key(auction_id, "forfeit", validate_round(round)?)
            }
            Intent::ConfirmWinner { round } => {
                settlement_key(auction_id, "confirm", validate_round(round)?)
            }
            Intent::Notify { template, round } => format!(
                "settle-auction/{auction_id}:notify:{}:{}",
                template.key_segment(),
                validate_round(round)?
            ),
        };
        Ok(Self(key))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IntentKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IntentKeyError {
    #[error("auction id cannot be used in an intent key")]
    InvalidAuctionId,
    #[error("settlement round must be positive")]
    InvalidRound,
}

fn settlement_key(auction_id: &str, operation: &str, round: u32) -> String {
    format!("settle-auction/{auction_id}:{operation}:{round}")
}

fn validate_round(round: u32) -> Result<u32, IntentKeyError> {
    (round > 0)
        .then_some(round)
        .ok_or(IntentKeyError::InvalidRound)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_every_grpc_code_and_retries_only_transient_failures() {
        let cases = [
            (Code::Ok, FailureClass::Fatal),
            (Code::Cancelled, FailureClass::Fatal),
            (Code::Unknown, FailureClass::Fatal),
            (Code::InvalidArgument, FailureClass::ExpectedRejection),
            (Code::DeadlineExceeded, FailureClass::Retryable),
            (Code::NotFound, FailureClass::ExpectedRejection),
            (Code::AlreadyExists, FailureClass::Fatal),
            (Code::PermissionDenied, FailureClass::ExpectedRejection),
            (Code::ResourceExhausted, FailureClass::Retryable),
            (Code::FailedPrecondition, FailureClass::ExpectedRejection),
            (Code::Aborted, FailureClass::Fatal),
            (Code::OutOfRange, FailureClass::Fatal),
            (Code::Unimplemented, FailureClass::Fatal),
            (Code::Internal, FailureClass::Fatal),
            (Code::Unavailable, FailureClass::Retryable),
            (Code::DataLoss, FailureClass::Fatal),
            (Code::Unauthenticated, FailureClass::Fatal),
        ];
        for (code, expected) in cases {
            assert_eq!(classify(code), expected, "unexpected class for {code:?}");
        }
    }

    #[test]
    fn activity_policy_is_bounded_and_exponential() {
        let policy = ActivityPolicy::default();
        assert_eq!(policy.start_to_close_timeout, Duration::from_secs(3));
        assert_eq!(policy.retry.maximum_attempts, 4);
        assert_eq!(policy.retry.backoff_coefficient, 2);
        assert!(policy.retry.jitter);
    }

    #[test]
    fn generates_every_stable_intent_and_notification_key() {
        let cases = [
            (Intent::CloseAuction, "auction-lifecycle/auction-1:close"),
            (
                Intent::AttemptPayment { round: 2 },
                "settle-auction/auction-1:attempt-payment:2",
            ),
            (
                Intent::ExpirePayment { round: 2 },
                "settle-auction/auction-1:expire-payment:2",
            ),
            (
                Intent::ForfeitSettlementCandidate { round: 2 },
                "settle-auction/auction-1:forfeit:2",
            ),
            (
                Intent::ConfirmWinner { round: 2 },
                "settle-auction/auction-1:confirm:2",
            ),
            (
                Intent::Notify {
                    template: NotificationTemplate::Settlement,
                    round: 2,
                },
                "settle-auction/auction-1:notify:settlement:2",
            ),
            (
                Intent::Notify {
                    template: NotificationTemplate::CandidateFailed,
                    round: 2,
                },
                "settle-auction/auction-1:notify:candidate-failed:2",
            ),
            (
                Intent::Notify {
                    template: NotificationTemplate::Recovery,
                    round: 2,
                },
                "settle-auction/auction-1:notify:recovery:2",
            ),
            (
                Intent::Notify {
                    template: NotificationTemplate::Winner,
                    round: 2,
                },
                "settle-auction/auction-1:notify:winner:2",
            ),
        ];
        for (intent, expected) in cases {
            assert_eq!(
                IntentKey::for_auction("auction-1", intent)
                    .unwrap()
                    .as_str(),
                expected
            );
        }
    }

    #[test]
    fn keys_separate_rounds_and_have_no_run_id_input() {
        let round_one =
            IntentKey::for_auction("auction-1", Intent::AttemptPayment { round: 1 }).unwrap();
        let replayed =
            IntentKey::for_auction("auction-1", Intent::AttemptPayment { round: 1 }).unwrap();
        let round_two =
            IntentKey::for_auction("auction-1", Intent::AttemptPayment { round: 2 }).unwrap();
        assert_eq!(round_one, replayed);
        assert_ne!(round_one, round_two);
        assert!(!round_one.as_str().contains("run"));
        assert!(IntentKey::for_auction("auction-1", Intent::AttemptPayment { round: 0 }).is_err());
    }
}
