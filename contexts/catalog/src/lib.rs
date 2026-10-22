//! Catalog bounded-context interfaces.

pub mod adapters;
pub mod application;
pub mod bootstrap;
pub mod domain;
pub mod telemetry;

#[allow(clippy::result_large_err)]
pub mod rpc {
    tonic::include_proto!("gavel.catalog.v1");
}
