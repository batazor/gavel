use crate::{
    application::{Clock, IdGenerator, PortError, Telemetry},
    domain::{PaymentAttemptId, PaymentInstrumentId},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UuidIds;
impl IdGenerator for UuidIds {
    fn instrument_id(&self) -> Result<PaymentInstrumentId, PortError> {
        PaymentInstrumentId::parse(format!("pi_{}", uuid::Uuid::now_v7().simple()))
            .map_err(|_| PortError::Internal)
    }
    fn attempt_id(&self) -> Result<PaymentAttemptId, PortError> {
        PaymentAttemptId::parse(format!("pa_{}", uuid::Uuid::now_v7().simple()))
            .map_err(|_| PortError::Internal)
    }
    fn event_id(&self) -> Result<String, PortError> {
        Ok(format!("pe_{}", uuid::Uuid::now_v7().simple()))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MetricsTelemetry;
impl Telemetry for MetricsTelemetry {
    fn command(&self, command: &'static str, outcome: &'static str) {
        metrics::counter!("gavel_payments_commands_total", "command" => command, "outcome" => outcome).increment(1);
    }
}
