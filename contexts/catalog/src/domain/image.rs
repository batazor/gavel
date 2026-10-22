use super::{AltText, DomainError, ImageId, LotId, ObjectMetadata, PrincipalId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageState {
    Pending,
    Ready {
        ready_at_millis: u64,
    },
    Attached {
        lot_id: LotId,
        position: u8,
        alt: AltText,
    },
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActualObjectMetadata {
    pub content_type: String,
    pub content_length: u64,
    pub checksum_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LotImage {
    image_id: ImageId,
    owner: PrincipalId,
    object_key: String,
    expected: ObjectMetadata,
    expires_at_millis: u64,
    state: ImageState,
}

impl LotImage {
    pub fn pending(
        image_id: ImageId,
        owner: PrincipalId,
        object_key: impl Into<String>,
        expected: ObjectMetadata,
        expires_at_millis: u64,
    ) -> Result<Self, DomainError> {
        let object_key = object_key.into();
        if object_key.is_empty() || expires_at_millis == 0 {
            return Err(DomainError::InvalidObjectMetadata);
        }
        Ok(Self {
            image_id,
            owner,
            object_key,
            expected,
            expires_at_millis,
            state: ImageState::Pending,
        })
    }

    pub fn restore(
        image_id: ImageId,
        owner: PrincipalId,
        object_key: impl Into<String>,
        expected: ObjectMetadata,
        expires_at_millis: u64,
        state: ImageState,
    ) -> Result<Self, DomainError> {
        let object_key = object_key.into();
        let valid_state = match &state {
            ImageState::Pending | ImageState::Invalid => true,
            ImageState::Ready { ready_at_millis } => *ready_at_millis > 0,
            ImageState::Attached { position, .. } => *position < 20,
        };
        if object_key.is_empty() || expires_at_millis == 0 || !valid_state {
            return Err(DomainError::InvalidObjectMetadata);
        }
        Ok(Self {
            image_id,
            owner,
            object_key,
            expected,
            expires_at_millis,
            state,
        })
    }

    pub fn verify(
        &mut self,
        caller: &PrincipalId,
        actual: &ActualObjectMetadata,
        now_millis: u64,
    ) -> Result<(), DomainError> {
        if caller != &self.owner {
            return Err(DomainError::ForeignImage);
        }
        match &self.state {
            ImageState::Ready { .. } => return Ok(()),
            ImageState::Pending => {}
            ImageState::Attached { .. } | ImageState::Invalid => {
                return Err(DomainError::InvalidImageTransition);
            }
        }
        if now_millis > self.expires_at_millis
            || actual.content_type != self.expected.content_type()
            || actual.content_length != self.expected.content_length()
            || actual.checksum_sha256 != self.expected.checksum_sha256()
        {
            self.state = ImageState::Invalid;
            return Err(DomainError::InvalidObjectMetadata);
        }
        self.state = ImageState::Ready {
            ready_at_millis: now_millis,
        };
        Ok(())
    }

    pub fn invalidate(&mut self, caller: &PrincipalId) -> Result<(), DomainError> {
        if caller != &self.owner {
            return Err(DomainError::ForeignImage);
        }
        if matches!(self.state, ImageState::Attached { .. }) {
            return Err(DomainError::InvalidImageTransition);
        }
        self.state = ImageState::Invalid;
        Ok(())
    }

    pub fn attach(
        &mut self,
        owner: &PrincipalId,
        lot_id: LotId,
        position: usize,
        alt: AltText,
    ) -> Result<(), DomainError> {
        if owner != &self.owner {
            return Err(DomainError::ForeignImage);
        }
        if !matches!(self.state, ImageState::Ready { .. }) || position >= 20 {
            return Err(DomainError::InvalidImageTransition);
        }
        self.state = ImageState::Attached {
            lot_id,
            position: position as u8,
            alt,
        };
        Ok(())
    }

    pub fn eligible_for_cleanup(&self, now_millis: u64, ready_ttl_millis: u64) -> bool {
        match self.state {
            ImageState::Pending => now_millis > self.expires_at_millis,
            ImageState::Ready { ready_at_millis } => {
                now_millis.saturating_sub(ready_at_millis) > ready_ttl_millis
            }
            ImageState::Invalid => true,
            ImageState::Attached { .. } => false,
        }
    }

    pub fn image_id(&self) -> &ImageId {
        &self.image_id
    }

    pub fn owner(&self) -> &PrincipalId {
        &self.owner
    }

    pub fn object_key(&self) -> &str {
        &self.object_key
    }

    pub const fn expected(&self) -> &ObjectMetadata {
        &self.expected
    }

    pub const fn expires_at_millis(&self) -> u64 {
        self.expires_at_millis
    }

    pub const fn state(&self) -> &ImageState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};

    use super::*;

    fn image(owner: &str) -> LotImage {
        LotImage::pending(
            ImageId::new("image-1").unwrap(),
            PrincipalId::new(owner).unwrap(),
            "opaque/object/key",
            ObjectMetadata::new("image/png", 42, STANDARD.encode([3_u8; 32])).unwrap(),
            1_000,
        )
        .unwrap()
    }

    fn actual() -> ActualObjectMetadata {
        ActualObjectMetadata {
            content_type: "image/png".to_owned(),
            content_length: 42,
            checksum_sha256: STANDARD.encode([3_u8; 32]),
        }
    }

    #[test]
    fn pending_verifies_then_attaches_once_for_the_owner() {
        let owner = PrincipalId::new("seller-1").unwrap();
        let mut image = image("seller-1");
        image.verify(&owner, &actual(), 500).unwrap();
        assert!(matches!(image.state(), ImageState::Ready { .. }));
        image
            .attach(
                &owner,
                LotId::new("lot-1").unwrap(),
                0,
                AltText::new("front").unwrap(),
            )
            .unwrap();
        assert!(matches!(image.state(), ImageState::Attached { .. }));
        assert!(
            image
                .attach(
                    &owner,
                    LotId::new("lot-2").unwrap(),
                    0,
                    AltText::new("again").unwrap(),
                )
                .is_err()
        );
        assert!(!image.eligible_for_cleanup(u64::MAX, 1));
    }

    #[test]
    fn foreign_mismatch_and_expiry_never_produce_ready() {
        let mut foreign = image("seller-1");
        assert_eq!(
            foreign.verify(&PrincipalId::new("seller-2").unwrap(), &actual(), 500),
            Err(DomainError::ForeignImage)
        );
        let mut mismatch = image("seller-1");
        let mut wrong = actual();
        wrong.content_length = 43;
        assert!(
            mismatch
                .verify(&PrincipalId::new("seller-1").unwrap(), &wrong, 500)
                .is_err()
        );
        assert!(matches!(mismatch.state(), ImageState::Invalid));
        let mut expired = image("seller-1");
        assert!(
            expired
                .verify(&PrincipalId::new("seller-1").unwrap(), &actual(), 1_001)
                .is_err()
        );
        assert!(expired.eligible_for_cleanup(1_001, 100));
    }
}
