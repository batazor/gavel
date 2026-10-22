//! Process-layer contracts and policies.

pub mod rpc_policy;

/// Owner-generated clients used by Temporal activities.
pub mod clients {
    pub use gavel_auction::rpc::auction_service_client::AuctionServiceClient;
    pub use gavel_catalog::rpc::catalog_service_client::CatalogServiceClient;
    pub use gavel_notify::rpc::notify_service_client::NotifyServiceClient;
    pub use gavel_payments_tonic::gavel::payments::v1::tonic::payments_service_client::PaymentsServiceClient;
}

/// Payment messages come from the same immutable BSR module commit as the client.
pub mod payments_contract {
    pub use gavel_payments_prost::gavel::payments::v1::*;
}
