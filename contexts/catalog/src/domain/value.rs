use std::{collections::HashSet, fmt};

use base64::{Engine, engine::general_purpose::STANDARD};

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("identifier is invalid")]
    InvalidIdentifier,
    #[error("text is invalid")]
    InvalidText,
    #[error("money is invalid")]
    InvalidMoney,
    #[error("closing policy is invalid")]
    InvalidClosingPolicy,
    #[error("object metadata is invalid")]
    InvalidObjectMetadata,
    #[error("image state transition is invalid")]
    InvalidImageTransition,
    #[error("image does not belong to the seller")]
    ForeignImage,
    #[error("image attachment batch is invalid")]
    InvalidImageBatch,
    #[error("lot state transition is invalid")]
    InvalidLotTransition,
    #[error("lot description is frozen")]
    DescriptionFrozen,
    #[error("aggregate version conflicts")]
    VersionConflict,
}

macro_rules! identifier {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 128
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                {
                    return Err(DomainError::InvalidIdentifier);
                }
                Ok(Self(value))
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

identifier!(LotId);
identifier!(ImageId);
identifier!(PrincipalId);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Title(String);

impl Title {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into().trim().to_owned();
        if value.is_empty() || value.chars().count() > 200 {
            return Err(DomainError::InvalidText);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Description(Option<String>);

impl Description {
    pub fn new(value: Option<String>) -> Result<Self, DomainError> {
        let value = value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        if value
            .as_ref()
            .is_some_and(|value| value.chars().count() > 5_000)
        {
            return Err(DomainError::InvalidText);
        }
        Ok(Self(value))
    }

    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AltText(String);

impl AltText {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into().trim().to_owned();
        if value.chars().count() > 500 {
            return Err(DomainError::InvalidText);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Currency(String);

impl Currency {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return Err(DomainError::InvalidMoney);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Money {
    minor: u64,
    currency: Currency,
}

impl Money {
    pub fn new(minor: u64, currency: Currency) -> Result<Self, DomainError> {
        if minor == 0 {
            return Err(DomainError::InvalidMoney);
        }
        Ok(Self { minor, currency })
    }

    pub const fn minor(&self) -> u64 {
        self.minor
    }

    pub const fn currency(&self) -> &Currency {
        &self.currency
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidStep(u64);

impl BidStep {
    pub fn new(minor: u64) -> Result<Self, DomainError> {
        if minor == 0 {
            return Err(DomainError::InvalidMoney);
        }
        Ok(Self(minor))
    }

    pub const fn minor(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosingPolicy {
    scheduled_end_millis: u64,
    inactivity_millis: Option<u64>,
    hard_deadline_millis: Option<u64>,
}

impl ClosingPolicy {
    pub fn new(
        scheduled_end_millis: u64,
        inactivity_millis: Option<u64>,
        hard_deadline_millis: Option<u64>,
    ) -> Result<Self, DomainError> {
        if scheduled_end_millis == 0
            || inactivity_millis == Some(0)
            || hard_deadline_millis.is_some_and(|value| value < scheduled_end_millis)
        {
            return Err(DomainError::InvalidClosingPolicy);
        }
        Ok(Self {
            scheduled_end_millis,
            inactivity_millis,
            hard_deadline_millis,
        })
    }

    pub const fn scheduled_end_millis(&self) -> u64 {
        self.scheduled_end_millis
    }

    pub const fn inactivity_millis(&self) -> Option<u64> {
        self.inactivity_millis
    }

    pub const fn hard_deadline_millis(&self) -> Option<u64> {
        self.hard_deadline_millis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectMetadata {
    content_type: String,
    content_length: u64,
    checksum_sha256: String,
}

impl ObjectMetadata {
    pub const MAX_CONTENT_LENGTH: u64 = 10 * 1024 * 1024;

    pub fn new(
        content_type: impl Into<String>,
        content_length: u64,
        checksum_sha256: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let content_type = content_type.into();
        let checksum_sha256 = checksum_sha256.into();
        let supported = matches!(
            content_type.as_str(),
            "image/jpeg" | "image/png" | "image/webp"
        );
        let digest_is_sha256 = STANDARD
            .decode(checksum_sha256.as_bytes())
            .is_ok_and(|digest| digest.len() == 32);
        if !supported
            || !(1..=Self::MAX_CONTENT_LENGTH).contains(&content_length)
            || !digest_is_sha256
        {
            return Err(DomainError::InvalidObjectMetadata);
        }
        Ok(Self {
            content_type,
            content_length,
            checksum_sha256,
        })
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub const fn content_length(&self) -> u64 {
        self.content_length
    }

    pub fn checksum_sha256(&self) -> &str {
        &self.checksum_sha256
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageAttachment {
    pub image_id: ImageId,
    pub alt: AltText,
}

pub fn validate_attachment_ids(values: &[ImageAttachment]) -> Result<(), DomainError> {
    if !(1..=20).contains(&values.len()) {
        return Err(DomainError::InvalidImageBatch);
    }
    let unique = values
        .iter()
        .map(|value| value.image_id.as_str())
        .collect::<HashSet<_>>();
    if unique.len() != values.len() {
        return Err(DomainError::InvalidImageBatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checksum() -> String {
        STANDARD.encode([7_u8; 32])
    }

    #[test]
    fn identifiers_and_text_are_bounded_and_normalized() {
        assert_eq!(LotId::new("lot_1").unwrap().as_str(), "lot_1");
        for invalid in ["", "has space", "x/y", &"x".repeat(129)] {
            assert_eq!(LotId::new(invalid), Err(DomainError::InvalidIdentifier));
        }
        assert_eq!(Title::new("  useful lot  ").unwrap().as_str(), "useful lot");
        assert!(Title::new(" ").is_err());
        assert!(Title::new("x".repeat(201)).is_err());
        assert_eq!(
            Description::new(Some("   ".to_owned())).unwrap().as_deref(),
            None
        );
        assert!(AltText::new("x".repeat(501)).is_err());
    }

    #[test]
    fn money_closing_policy_and_object_metadata_reject_invalid_boundaries() {
        assert!(Money::new(1, Currency::new("USD").unwrap()).is_ok());
        assert!(Currency::new("usd").is_err());
        assert!(Money::new(0, Currency::new("USD").unwrap()).is_err());
        assert!(BidStep::new(0).is_err());
        assert!(ClosingPolicy::new(1_000, Some(100), Some(2_000)).is_ok());
        assert!(ClosingPolicy::new(1_000, Some(0), None).is_err());
        assert!(ClosingPolicy::new(1_000, None, Some(999)).is_err());
        assert!(ObjectMetadata::new("image/webp", 10, checksum()).is_ok());
        assert!(ObjectMetadata::new("text/html", 10, checksum()).is_err());
        assert!(ObjectMetadata::new("image/png", 0, checksum()).is_err());
        assert!(ObjectMetadata::new("image/png", 10, "not-base64").is_err());
    }
}
