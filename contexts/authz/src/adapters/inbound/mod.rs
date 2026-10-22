//! Trusted inbound transport adapters.

mod error_mapping;
mod grpc;
mod workload_identity;

pub use error_mapping::{RpcError, RpcKind, map_rpc_error};
pub use grpc::AuthzGrpcService;
pub use workload_identity::{TrustedProxyCidr, WorkloadIdentityInterceptor};
