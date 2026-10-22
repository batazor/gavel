use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};

use crate::adapters::outbound::StripeSignatureVerifier;
use crate::{
    application::{
        ApplicationError, CommandFingerprint, PaymentApplication, VerifiedProviderEvent,
        VerifiedProviderOutcome,
    },
    domain::{ProviderReference, RedactedDisplay},
};

#[async_trait]
pub trait CallbackSink: Send + Sync {
    async fn accept(&self, raw_body: &[u8]) -> Result<(), CallbackFailure>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackFailure {
    Conflict,
    Retryable,
    Invalid,
}

pub struct StripeCallbackSink {
    application: Arc<PaymentApplication>,
    fingerprint: CommandFingerprint,
}
impl StripeCallbackSink {
    pub fn new(application: Arc<PaymentApplication>, fingerprint: CommandFingerprint) -> Self {
        Self {
            application,
            fingerprint,
        }
    }
}

#[derive(serde::Deserialize)]
struct StripeEvent {
    id: String,
    created: u64,
    #[serde(rename = "type")]
    kind: String,
    data: StripeEventData,
}
#[derive(serde::Deserialize)]
struct StripeEventData {
    object: StripeObject,
}
#[derive(serde::Deserialize)]
struct StripeObject {
    session_ref: Option<String>,
    customer_ref: Option<String>,
    instrument_ref: Option<String>,
    operation_ref: Option<String>,
    recovery_url: Option<String>,
    display: Option<String>,
}

#[async_trait]
impl CallbackSink for StripeCallbackSink {
    async fn accept(&self, raw_body: &[u8]) -> Result<(), CallbackFailure> {
        let parsed: StripeEvent =
            serde_json::from_slice(raw_body).map_err(|_| CallbackFailure::Invalid)?;
        let event_ref =
            ProviderReference::parse(parsed.id.as_str()).map_err(|_| CallbackFailure::Invalid)?;
        let reference = |value: Option<String>| {
            value.ok_or(CallbackFailure::Invalid).and_then(|value| {
                ProviderReference::parse(value).map_err(|_| CallbackFailure::Invalid)
            })
        };
        let outcome = match parsed.kind.as_str() {
            "instrument.ready" => VerifiedProviderOutcome::InstrumentReady {
                session_ref: reference(parsed.data.object.session_ref)?,
                customer_ref: reference(parsed.data.object.customer_ref)?,
                instrument_ref: reference(parsed.data.object.instrument_ref)?,
                display: RedactedDisplay::parse(
                    parsed.data.object.display.ok_or(CallbackFailure::Invalid)?,
                )
                .map_err(|_| CallbackFailure::Invalid)?,
            },
            "payment.processing" => VerifiedProviderOutcome::AttemptProcessing {
                operation_ref: reference(parsed.data.object.operation_ref)?,
            },
            "payment.recovery_required" => VerifiedProviderOutcome::AttemptRecoveryRequired {
                operation_ref: reference(parsed.data.object.operation_ref)?,
                recovery_url: parsed
                    .data
                    .object
                    .recovery_url
                    .ok_or(CallbackFailure::Invalid)?,
            },
            "payment.succeeded" => VerifiedProviderOutcome::AttemptSucceeded {
                operation_ref: reference(parsed.data.object.operation_ref)?,
            },
            _ => return Ok(()),
        };
        self.application
            .handle_provider_event(VerifiedProviderEvent {
                provider: "stripe",
                event_ref,
                event_ref_digest: self.fingerprint.digest(parsed.id.as_bytes()),
                fingerprint: self.fingerprint.digest(raw_body),
                occurred_at_millis: parsed.created.saturating_mul(1000),
                outcome,
            })
            .await
            .map_err(map_application)
    }
}

fn map_application(error: ApplicationError) -> CallbackFailure {
    match error {
        ApplicationError::IdempotencyConflict => CallbackFailure::Conflict,
        ApplicationError::DependencyUnavailable
        | ApplicationError::InProgress
        | ApplicationError::Internal => CallbackFailure::Retryable,
        _ => CallbackFailure::Invalid,
    }
}

pub struct CallbackIngress {
    pub verifier: StripeSignatureVerifier,
    pub sink: Arc<dyn CallbackSink>,
    pub now_secs: Arc<dyn Fn() -> u64 + Send + Sync>,
}

pub fn callback_router(ingress: Arc<CallbackIngress>) -> Router {
    Router::new()
        .route("/payment-provider-callbacks/stripe", post(handle))
        .with_state(ingress)
}

async fn handle(
    State(ingress): State<Arc<CallbackIngress>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let Some(signature) = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::BAD_REQUEST;
    };
    if ingress
        .verifier
        .verify(signature, &body, (ingress.now_secs)())
        .is_err()
    {
        metrics::counter!("gavel_payments_callback_total", "outcome" => "invalid_signature")
            .increment(1);
        return StatusCode::BAD_REQUEST;
    }
    match ingress.sink.accept(&body).await {
        Ok(()) => {
            metrics::counter!("gavel_payments_callback_total", "outcome" => "accepted")
                .increment(1);
            StatusCode::OK
        }
        Err(CallbackFailure::Conflict | CallbackFailure::Invalid) => StatusCode::BAD_REQUEST,
        Err(CallbackFailure::Retryable) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
