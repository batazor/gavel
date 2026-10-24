//! I/O adapters for owner RPC, Temporal and Redpanda delivery.

pub mod owner_clients {
    pub use gavel_auction::rpc::auction_service_client::AuctionServiceClient;
    pub use gavel_catalog::rpc::catalog_service_client::CatalogServiceClient;
    pub use gavel_notify::rpc::notify_service_client::NotifyServiceClient;
    pub use gavel_payments_tonic::gavel::payments::v1::tonic::payments_service_client::PaymentsServiceClient;
}

pub mod owner_rpc;
pub mod redpanda;
pub mod temporal_signal;
