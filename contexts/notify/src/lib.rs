//! Notify bounded-context interfaces.

#[allow(clippy::result_large_err)]
pub mod rpc {
    tonic::include_proto!("gavel.notify.v1");
}
