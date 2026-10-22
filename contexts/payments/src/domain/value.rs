use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("value is empty")]
    Empty,
    #[error("value is too long")]
    TooLong,
    #[error("value has an invalid format")]
    InvalidFormat,
    #[error("currency is unsupported")]
    UnsupportedCurrency,
    #[error("amount must be positive")]
    InvalidAmount,
    #[error("settlement round must be positive")]
    InvalidRound,
    #[error("transition is not allowed")]
    InvalidTransition,
    #[error("aggregate version overflow")]
    VersionOverflow,
}

macro_rules! validated_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                let valid = !value.is_empty()
                    && value.len() <= 128
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'/')
                    });
                if valid {
                    Ok(Self(value))
                } else {
                    Err(DomainError::InvalidFormat)
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

validated_id!(AuctionId);
validated_id!(CandidateId);
validated_id!(PaymentAttemptId);
validated_id!(PaymentInstrumentId);
validated_id!(PrincipalId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SettlementRound(u32);

impl SettlementRound {
    pub fn new(value: u32) -> Result<Self, DomainError> {
        (value > 0)
            .then_some(Self(value))
            .ok_or(DomainError::InvalidRound)
    }
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
    USD,
    EUR,
    GBP,
}

impl FromStr for Currency {
    type Err = DomainError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_uppercase().as_str() {
            "USD" => Ok(Self::USD),
            "EUR" => Ok(Self::EUR),
            "GBP" => Ok(Self::GBP),
            _ => Err(DomainError::UnsupportedCurrency),
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::USD => "USD",
            Self::EUR => "EUR",
            Self::GBP => "GBP",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    minor_units: u64,
    currency: Currency,
}

impl Money {
    pub fn new(minor_units: u64, currency: Currency) -> Result<Self, DomainError> {
        (minor_units > 0)
            .then_some(Self {
                minor_units,
                currency,
            })
            .ok_or(DomainError::InvalidAmount)
    }
    pub const fn minor_units(&self) -> u64 {
        self.minor_units
    }
    pub const fn currency(&self) -> Currency {
        self.currency
    }
}

macro_rules! private_value {
    ($name:ident, $validator:expr) => {
        #[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                if ($validator)(&value) {
                    Ok(Self(value))
                } else {
                    Err(DomainError::InvalidFormat)
                }
            }
            pub fn expose(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "([REDACTED])"))
            }
        }
    };
}

private_value!(EmailAddress, |value: &String| value.len() <= 254
    && value.split_once('@').is_some_and(|(local, domain)| !local
        .is_empty()
        && domain.contains('.')));
private_value!(ProviderReference, |value: &String| !value.is_empty()
    && value.len() <= 255
    && !value.chars().any(char::is_whitespace));

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TermsVersion(String);
impl TermsVersion {
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if !value.is_empty() && value.len() <= 64 {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidFormat)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RedactedDisplay(String);
impl RedactedDisplay {
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let digit_count = value.chars().filter(char::is_ascii_digit).count();
        if !value.is_empty() && value.len() <= 32 && digit_count <= 4 {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidFormat)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
