use super::{
    BidStep, ClosingPolicy, Description, DomainError, ImageAttachment, ImageState, LotId, LotImage,
    Money, PrincipalId, Title, value::validate_attachment_ids,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LotState {
    Listed,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lot {
    lot_id: LotId,
    seller: PrincipalId,
    title: Title,
    description: Description,
    start_price: Money,
    bid_step: BidStep,
    closing_policy: ClosingPolicy,
    state: LotState,
    version: u64,
    authz_revision: String,
    description_frozen: bool,
    auction_source_version: u64,
    listed_at_millis: u64,
}

impl Lot {
    #[allow(clippy::too_many_arguments)]
    pub fn list(
        lot_id: LotId,
        seller: PrincipalId,
        title: Title,
        description: Description,
        start_price: Money,
        bid_step: BidStep,
        closing_policy: ClosingPolicy,
        authz_revision: impl Into<String>,
        listed_at_millis: u64,
    ) -> Result<Self, DomainError> {
        let authz_revision = authz_revision.into();
        if authz_revision.is_empty() || listed_at_millis == 0 {
            return Err(DomainError::InvalidLotTransition);
        }
        Ok(Self {
            lot_id,
            seller,
            title,
            description,
            start_price,
            bid_step,
            closing_policy,
            state: LotState::Listed,
            version: 1,
            authz_revision,
            description_frozen: false,
            auction_source_version: 0,
            listed_at_millis,
        })
    }

    pub fn update(
        &mut self,
        expected_version: u64,
        title: Option<Title>,
        description: Option<Description>,
    ) -> Result<u64, DomainError> {
        if expected_version != self.version {
            return Err(DomainError::VersionConflict);
        }
        if self.state != LotState::Listed {
            return Err(DomainError::InvalidLotTransition);
        }
        if self.description_frozen {
            return Err(DomainError::DescriptionFrozen);
        }
        if title.is_none() && description.is_none() {
            return Err(DomainError::InvalidText);
        }
        if let Some(title) = title {
            self.title = title;
        }
        if let Some(description) = description {
            self.description = description;
        }
        self.version += 1;
        Ok(self.version)
    }

    pub fn freeze_description(&mut self, source_version: u64) -> bool {
        if source_version <= self.auction_source_version {
            return false;
        }
        self.auction_source_version = source_version;
        self.description_frozen = true;
        true
    }

    pub fn withdraw(&mut self, expected_version: u64) -> Result<u64, DomainError> {
        if expected_version != self.version {
            return Err(DomainError::VersionConflict);
        }
        match self.state {
            LotState::Listed => {
                self.state = LotState::Withdrawn;
                self.version += 1;
                Ok(self.version)
            }
            LotState::Withdrawn => Ok(self.version),
        }
    }

    pub fn lot_id(&self) -> &LotId {
        &self.lot_id
    }

    pub fn seller(&self) -> &PrincipalId {
        &self.seller
    }

    pub fn title(&self) -> &Title {
        &self.title
    }

    pub fn description(&self) -> &Description {
        &self.description
    }

    pub const fn start_price(&self) -> &Money {
        &self.start_price
    }

    pub const fn bid_step(&self) -> BidStep {
        self.bid_step
    }

    pub const fn closing_policy(&self) -> &ClosingPolicy {
        &self.closing_policy
    }

    pub const fn state(&self) -> LotState {
        self.state
    }

    pub const fn version(&self) -> u64 {
        self.version
    }

    pub fn authz_revision(&self) -> &str {
        &self.authz_revision
    }

    pub const fn description_frozen(&self) -> bool {
        self.description_frozen
    }

    pub const fn listed_at_millis(&self) -> u64 {
        self.listed_at_millis
    }
}

pub fn attach_images(
    images: &mut [LotImage],
    requested: &[ImageAttachment],
    seller: &PrincipalId,
    lot_id: &LotId,
) -> Result<(), DomainError> {
    validate_attachment_ids(requested)?;
    for requested_image in requested {
        let image = images
            .iter()
            .find(|image| image.image_id() == &requested_image.image_id)
            .ok_or(DomainError::InvalidImageBatch)?;
        if image.owner() != seller || !matches!(image.state(), ImageState::Ready { .. }) {
            return Err(DomainError::InvalidImageBatch);
        }
    }
    for (position, requested_image) in requested.iter().enumerate() {
        let image = images
            .iter_mut()
            .find(|image| image.image_id() == &requested_image.image_id)
            .expect("validated image must remain present");
        image.attach(
            seller,
            lot_id.clone(),
            position,
            requested_image.alt.clone(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};

    use super::*;
    use crate::domain::{ActualObjectMetadata, ImageId, ObjectMetadata};

    fn lot() -> Lot {
        Lot::list(
            LotId::new("lot-1").unwrap(),
            PrincipalId::new("seller-1").unwrap(),
            Title::new("Vintage clock").unwrap(),
            Description::new(Some("Works".to_owned())).unwrap(),
            Money::new(100, crate::domain::Currency::new("USD").unwrap()).unwrap(),
            BidStep::new(10).unwrap(),
            ClosingPolicy::new(2_000, Some(100), Some(3_000)).unwrap(),
            "authz-revision",
            1_000,
        )
        .unwrap()
    }

    fn ready_image(id: &str, owner: &str) -> LotImage {
        let checksum = STANDARD.encode([2_u8; 32]);
        let mut image = LotImage::pending(
            ImageId::new(id).unwrap(),
            PrincipalId::new(owner).unwrap(),
            format!("objects/{id}"),
            ObjectMetadata::new("image/jpeg", 20, checksum.clone()).unwrap(),
            2_000,
        )
        .unwrap();
        image
            .verify(
                &PrincipalId::new(owner).unwrap(),
                &ActualObjectMetadata {
                    content_type: "image/jpeg".to_owned(),
                    content_length: 20,
                    checksum_sha256: checksum,
                },
                1_500,
            )
            .unwrap();
        image
    }

    #[test]
    fn lot_updates_freezes_monotonically_and_withdraws_once() {
        let mut lot = lot();
        assert_eq!(lot.version(), 1);
        assert_eq!(
            lot.update(1, Some(Title::new("New title").unwrap()), None),
            Ok(2)
        );
        assert!(lot.freeze_description(7));
        assert!(!lot.freeze_description(6));
        assert_eq!(
            lot.update(2, Some(Title::new("Too late").unwrap()), None),
            Err(DomainError::DescriptionFrozen)
        );
        assert_eq!(lot.withdraw(2), Ok(3));
        assert_eq!(lot.withdraw(3), Ok(3));
        assert_eq!(lot.state(), LotState::Withdrawn);
        assert_eq!(
            lot.update(3, None, None),
            Err(DomainError::InvalidLotTransition)
        );
    }

    #[test]
    fn attachment_batch_is_all_or_nothing_and_ordered() {
        let seller = PrincipalId::new("seller-1").unwrap();
        let lot_id = LotId::new("lot-1").unwrap();
        let mut images = vec![
            ready_image("image-1", "seller-1"),
            ready_image("image-2", "seller-1"),
        ];
        let request = vec![
            ImageAttachment {
                image_id: ImageId::new("image-2").unwrap(),
                alt: crate::domain::AltText::new("second first").unwrap(),
            },
            ImageAttachment {
                image_id: ImageId::new("image-1").unwrap(),
                alt: crate::domain::AltText::new("first second").unwrap(),
            },
        ];
        attach_images(&mut images, &request, &seller, &lot_id).unwrap();
        assert!(matches!(
            images[0].state(),
            ImageState::Attached { position: 1, .. }
        ));
        assert!(matches!(
            images[1].state(),
            ImageState::Attached { position: 0, .. }
        ));

        let mut invalid = vec![ready_image("image-3", "seller-1")];
        let before = invalid.clone();
        let duplicate = vec![
            ImageAttachment {
                image_id: ImageId::new("image-3").unwrap(),
                alt: crate::domain::AltText::new("").unwrap(),
            },
            ImageAttachment {
                image_id: ImageId::new("image-3").unwrap(),
                alt: crate::domain::AltText::new("").unwrap(),
            },
        ];
        assert!(attach_images(&mut invalid, &duplicate, &seller, &lot_id).is_err());
        assert_eq!(invalid, before);
    }

    #[test]
    fn empty_oversized_foreign_and_non_ready_batches_are_rejected_without_mutation() {
        let seller = PrincipalId::new("seller-1").unwrap();
        let lot_id = LotId::new("lot-1").unwrap();
        let cases = [
            Vec::new(),
            (0..21)
                .map(|index| ImageAttachment {
                    image_id: ImageId::new(format!("image-{index}")).unwrap(),
                    alt: crate::domain::AltText::new("").unwrap(),
                })
                .collect(),
        ];
        for request in cases {
            let mut images = vec![ready_image("image-0", "seller-1")];
            let before = images.clone();
            assert!(attach_images(&mut images, &request, &seller, &lot_id).is_err());
            assert_eq!(images, before);
        }

        let request = vec![ImageAttachment {
            image_id: ImageId::new("image-foreign").unwrap(),
            alt: crate::domain::AltText::new("").unwrap(),
        }];
        let mut foreign = vec![ready_image("image-foreign", "seller-2")];
        let before = foreign.clone();
        assert!(attach_images(&mut foreign, &request, &seller, &lot_id).is_err());
        assert_eq!(foreign, before);

        let checksum = STANDARD.encode([2_u8; 32]);
        let mut pending = vec![
            LotImage::pending(
                ImageId::new("image-pending").unwrap(),
                seller.clone(),
                "objects/pending",
                ObjectMetadata::new("image/jpeg", 20, checksum).unwrap(),
                2_000,
            )
            .unwrap(),
        ];
        let request = vec![ImageAttachment {
            image_id: ImageId::new("image-pending").unwrap(),
            alt: crate::domain::AltText::new("").unwrap(),
        }];
        let before = pending.clone();
        assert!(attach_images(&mut pending, &request, &seller, &lot_id).is_err());
        assert_eq!(pending, before);
    }
}
