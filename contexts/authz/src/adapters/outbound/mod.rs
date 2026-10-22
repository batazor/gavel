//! SpiceDB and Authz-owned persistence adapters.

mod postgres;
mod spicedb;

pub use postgres::PostgresReceiptStore;
pub use spicedb::{SpiceDbAdapter, SpiceDbAdapterError};
