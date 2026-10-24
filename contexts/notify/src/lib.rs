//! Notify bounded context.

pub mod adapters;
pub mod application;
pub mod bootstrap;
pub mod domain;

#[allow(clippy::result_large_err)]
pub mod rpc {
    tonic::include_proto!("gavel.notify.v1");
}
