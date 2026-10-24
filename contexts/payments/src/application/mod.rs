//! Payments application layer: use cases and provider-neutral ports.

mod error;
mod ports;
mod receipts;
mod service;

pub use error::{ApplicationError, ErrorCode};
pub use ports::*;
pub use receipts::{CommandFingerprint, ReceiptDecision, ReceiptExecutor};
pub use service::{
    AttemptPaymentCommand, BeginRegistrationCommand, ContinuePaymentCommand, ExpirePaymentCommand,
    GetSettlementAttemptQuery, PaymentApplication, PaymentAttemptView, PaymentInstrumentView,
    RevokeInstrumentCommand, SettlementAttemptData, SettlementAttemptView,
};
