//! Authz policy application and its owned transport/storage adapters.

pub mod adapters;
pub mod application;
pub mod bootstrap;
pub mod local_bootstrap;
pub mod telemetry;

#[allow(clippy::result_large_err)]
pub mod rpc {
    tonic::include_proto!("gavel.authz.v1");
}

#[allow(clippy::result_large_err)]
pub mod spicedb_rpc {
    tonic::include_proto!("authzed.api.v1");
}

/// Descriptor set used by compatibility-focused contract tests.
pub const AUTHZ_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("authz_descriptor");
