use std::{fmt, num::NonZeroU32};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProcessId(String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuctionId(String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateId(String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PaymentAttemptId(String);

macro_rules! process_id {
    ($type:ident) => {
        impl $type {
            pub fn parse(value: impl Into<String>) -> Result<Self, ProcessError> {
                let value = value.into();
                let valid = !value.is_empty()
                    && value.len() <= 128
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'/')
                    });
                valid
                    .then_some(Self(value))
                    .ok_or(ProcessError::InvalidInput)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

process_id!(ProcessId);
process_id!(AuctionId);
process_id!(CandidateId);
process_id!(PaymentAttemptId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SettlementRound(NonZeroU32);

impl SettlementRound {
    pub fn new(value: u32) -> Result<Self, ProcessError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(ProcessError::InvalidInput)
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    pub minor_units: u64,
    pub currency_code: String,
}

impl Money {
    pub fn new(minor_units: u64, currency_code: impl Into<String>) -> Result<Self, ProcessError> {
        let currency_code = currency_code.into();
        if minor_units == 0
            || currency_code.len() != 3
            || !currency_code.bytes().all(|byte| byte.is_ascii_uppercase())
        {
            return Err(ProcessError::InvalidInput);
        }
        Ok(Self {
            minor_units,
            currency_code,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrelationContext {
    pub correlation_id: String,
    pub causation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuctionLifecycleInput {
    pub process_id: ProcessId,
    pub auction_id: AuctionId,
    pub opened_version: u64,
    pub close_at_millis: u64,
    pub correlation: CorrelationContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettleAuctionInput {
    pub auction_id: AuctionId,
    pub correlation: CorrelationContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementCandidate {
    pub auction_id: AuctionId,
    pub candidate_id: CandidateId,
    pub round: SettlementRound,
    pub amount: Money,
    pub auction_version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementAttemptStatus {
    Processing,
    RecoveryRequired,
    Succeeded { payment_id: String },
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ProcessError {
    #[error("invalid process input")]
    InvalidInput,
    #[error("event is stale")]
    StaleEvent,
    #[error("redelivery payload changed")]
    ChangedRedelivery,
    #[error("owner rejected the intent: {code}")]
    OwnerRejected { code: String },
    #[error("dependency is unavailable")]
    DependencyUnavailable,
    #[error("process invariant was violated")]
    InvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_money_validate_without_transport_types() {
        assert!(AuctionId::parse("auction-1").is_ok());
        assert!(AuctionId::parse("auction 1").is_err());
        assert_eq!(SettlementRound::new(2).unwrap().get(), 2);
        assert!(SettlementRound::new(0).is_err());
        assert!(Money::new(100, "USD").is_ok());
        assert!(Money::new(0, "USD").is_err());
    }
}
