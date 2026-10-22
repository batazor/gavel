//! Pure domain model for the Auction bounded context.
//!
//! Infrastructure adapters are expected to load an [`Auction`] from its private
//! event stream, call [`Auction::decide`], and atomically append the returned
//! private events and public events using `Auction::version()` as the optimistic
//! lock.

pub mod adapters;
pub mod application;
pub mod bootstrap;
pub mod domain;
pub mod telemetry;

#[allow(clippy::result_large_err)]
pub mod rpc {
    tonic::include_proto!("gavel.auction.v1");
}

#[allow(clippy::result_large_err)]
pub mod authz_rpc {
    tonic::include_proto!("gavel.authz.v1");
}

/// Descriptor set for contract-level privacy and compatibility tests.
pub const AUCTION_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("auction_descriptor");

pub use domain::{
    Auction, AuctionCommand, AuctionError, AuctionEvent, AuctionId, AuctionStatus, BidId,
    BidderAlias, BidderId, ClosingPolicy, Decision, DurationMillis, LotId, Money, PaymentId,
    PublicEvent, SellerId, Timestamp,
};
