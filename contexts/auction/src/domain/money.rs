use std::{error::Error, fmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Money {
    amount_minor: u64,
    currency: String,
}

impl Money {
    pub fn new(amount_minor: u64, currency: impl Into<String>) -> Result<Self, MoneyError> {
        let currency = currency.into();
        if currency.len() != 3
            || !currency
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_uppercase())
        {
            return Err(MoneyError::InvalidCurrency(currency));
        }

        Ok(Self {
            amount_minor,
            currency,
        })
    }

    pub const fn amount_minor(&self) -> u64 {
        self.amount_minor
    }

    pub fn currency(&self) -> &str {
        &self.currency
    }

    pub fn same_currency(&self, other: &Self) -> bool {
        self.currency == other.currency
    }

    pub(crate) fn with_amount(&self, amount_minor: u64) -> Self {
        Self {
            amount_minor,
            currency: self.currency.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoneyError {
    InvalidCurrency(String),
}

impl fmt::Display for MoneyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCurrency(currency) => {
                write!(
                    formatter,
                    "currency must be a three-letter uppercase code: {currency}"
                )
            }
        }
    }
}

impl Error for MoneyError {}
