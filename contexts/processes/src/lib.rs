//! Process-layer contracts, deterministic DTOs and integration boundaries.

pub mod activities;
pub mod adapters;
pub mod bootstrap;
pub mod ingress;
pub mod rpc_policy;
pub mod telemetry;
pub mod workflows;

/// Owner-generated clients used by Temporal activities.
pub mod clients {
    pub use crate::adapters::owner_clients::*;
}

/// Payment messages come from the same immutable BSR module commit as the client.
pub mod payments_contract {
    pub use gavel_payments_prost::gavel::payments::v1::*;
}
