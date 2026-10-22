//! Pure Catalog domain model.

mod image;
mod lot;
mod value;

pub use image::{ActualObjectMetadata, ImageState, LotImage};
pub use lot::{Lot, LotState, attach_images};
pub use value::{
    AltText, BidStep, ClosingPolicy, Currency, Description, DomainError, ImageAttachment, ImageId,
    LotId, Money, ObjectMetadata, PrincipalId, Title,
};
