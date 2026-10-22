//! Auction use cases and the ports they require.
//!
//! This layer may depend on [`crate::domain`], but never on concrete adapters.

mod dto;
mod error;
mod executor;
mod ports;
pub mod testing;
mod use_cases;

pub use dto::{
    AuctionView, CancelAuctionInput, CloseAuctionInput, CommandContext, ConfirmWinnerInput,
    CurrentPriceView, ForfeitSettlementCandidateInput, LifecycleResult, MoneyView, PlaceBidInput,
    PlaceBidResult, Principal, PrincipalKind, PublicAuctionStatus, RescheduleAuctionInput,
};
pub use error::{ApplicationError, ErrorCode};
pub use executor::{CommandExecutor, PreparedCommand};
pub use ports::{
    AuctionCache, AuctionProjection, AuctionProjectionReader, AuctionUnitOfWork, Authorization,
    AuthorizationDecision, CacheError, CommandIdentity, CommandLookup, CommandResult, CommitError,
    CommitOutcome, CommitRequest, Inbox, InboxError, InboxOutcome, LoadedAuction, PaymentReadiness,
    PaymentReadinessStore, PaymentReadinessUpdate, PaymentReadinessUpdateOutcome, PermissionCheck,
    PortError, PriceSnapshot, ReconciliationOperation, ReconciliationQueue, RelationshipWrite,
};
pub use use_cases::RescheduleAuction;
pub use use_cases::{
    AuctionLifecycle, LotListed, OpenAuction, PlaceBid, ReconciliationReport, ReconciliationWorker,
};
