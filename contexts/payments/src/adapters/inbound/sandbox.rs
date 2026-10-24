use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use crate::{
    adapters::outbound::{SandboxGateway, SandboxOutcome},
    application::{
        ExpirePaymentCommand, PaymentApplication, VerifiedProviderEvent, VerifiedProviderOutcome,
    },
    domain::{ProviderReference, RedactedDisplay},
};

struct SandboxControlState {
    application: Arc<PaymentApplication>,
    gateway: Arc<SandboxGateway>,
    control_token: String,
}

#[derive(Debug, serde::Deserialize)]
struct SetOutcomeBody {
    outcome: SandboxOutcome,
}

#[derive(Debug, serde::Serialize)]
struct ControlResult {
    accepted: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AttemptStatusResult {
    attempt_id: String,
    auction_id: String,
    settlement_round: u32,
    status: crate::domain::PaymentAttemptState,
    version: u64,
}

/// Mounted only by the guarded local sandbox runtime profile.
pub fn sandbox_control_router(
    application: Arc<PaymentApplication>,
    gateway: Arc<SandboxGateway>,
    control_token: String,
) -> Router {
    let state = Arc::new(SandboxControlState {
        application,
        gateway,
        control_token,
    });
    Router::new()
        .route(
            "/local/sandbox/outcomes/{auction_id}/{round}",
            put(set_outcome),
        )
        .route(
            "/local/sandbox/instruments/{session_ref}/ready",
            post(mark_instrument_ready),
        )
        .route(
            "/local/sandbox/attempts/{auction_id}/{round}/expire",
            post(expire_attempt),
        )
        .route(
            "/local/sandbox/attempts/{auction_id}/{round}",
            get(attempt_status),
        )
        .with_state(state)
}

async fn attempt_status(
    State(state): State<Arc<SandboxControlState>>,
    headers: HeaderMap,
    Path((auction_id, round)): Path<(String, u32)>,
) -> Result<Json<AttemptStatusResult>, StatusCode> {
    authorize(&headers, &state.control_token)?;
    if auction_id.trim().is_empty() || round == 0 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let attempt = state
        .application
        .uow
        .attempt_for_round(&auction_id, round)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(AttemptStatusResult {
        attempt_id: attempt.id().to_string(),
        auction_id: attempt.auction_id().to_string(),
        settlement_round: attempt.settlement_round().get(),
        status: attempt.state(),
        version: attempt.version(),
    }))
}

async fn expire_attempt(
    State(state): State<Arc<SandboxControlState>>,
    headers: HeaderMap,
    Path((auction_id, round)): Path<(String, u32)>,
) -> Result<Json<ControlResult>, StatusCode> {
    authorize(&headers, &state.control_token)?;
    if auction_id.trim().is_empty() || round == 0 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let attempt = state
        .application
        .uow
        .attempt_for_round(&auction_id, round)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .ok_or(StatusCode::NOT_FOUND)?;
    state
        .application
        .expire_payment(ExpirePaymentCommand {
            workload_id: "processes".to_owned(),
            attempt_id: attempt.id().to_string(),
            idempotency_key: format!("sandbox-expire-{auction_id}-{round}"),
            consistency_token: None,
        })
        .await
        .map_err(|error| match error {
            crate::application::ApplicationError::NotFound => StatusCode::NOT_FOUND,
            crate::application::ApplicationError::AlreadySucceeded
            | crate::application::ApplicationError::IdempotencyConflict => StatusCode::CONFLICT,
            crate::application::ApplicationError::DependencyUnavailable
            | crate::application::ApplicationError::InProgress => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::BAD_REQUEST,
        })?;
    Ok(Json(ControlResult { accepted: true }))
}

async fn set_outcome(
    State(state): State<Arc<SandboxControlState>>,
    headers: HeaderMap,
    Path((auction_id, round)): Path<(String, u32)>,
    Json(body): Json<SetOutcomeBody>,
) -> Result<Json<ControlResult>, StatusCode> {
    authorize(&headers, &state.control_token)?;
    if auction_id.trim().is_empty() || round == 0 {
        return Err(StatusCode::BAD_REQUEST);
    }
    state
        .gateway
        .set_outcome(&auction_id, round, body.outcome)
        .await;
    Ok(Json(ControlResult { accepted: true }))
}

async fn mark_instrument_ready(
    State(state): State<Arc<SandboxControlState>>,
    headers: HeaderMap,
    Path(session_ref): Path<String>,
) -> Result<Json<ControlResult>, StatusCode> {
    authorize(&headers, &state.control_token)?;
    let session_ref = ProviderReference::parse(session_ref).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (customer_ref, instrument_ref) = SandboxGateway::registration_refs(session_ref.expose())
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let identity = digest(session_ref.expose().as_bytes());
    state
        .application
        .handle_provider_event(VerifiedProviderEvent {
            provider: "sandbox",
            event_ref: ProviderReference::parse(format!("sandbox_ready_{identity}"))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            event_ref_digest: digest(format!("ready:{identity}").as_bytes()),
            fingerprint: digest(format!("instrument-ready:{identity}").as_bytes()),
            occurred_at_millis: u64::try_from(chrono::Utc::now().timestamp_millis())
                .unwrap_or_default(),
            outcome: VerifiedProviderOutcome::InstrumentReady {
                session_ref,
                customer_ref,
                instrument_ref,
                display: RedactedDisplay::parse("Sandbox •••• 4242")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            },
        })
        .await
        .map_err(|error| match error {
            crate::application::ApplicationError::NotFound => StatusCode::NOT_FOUND,
            crate::application::ApplicationError::IdempotencyConflict => StatusCode::CONFLICT,
            crate::application::ApplicationError::DependencyUnavailable
            | crate::application::ApplicationError::InProgress => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::BAD_REQUEST,
        })?;
    Ok(Json(ControlResult { accepted: true }))
}

fn authorize(headers: &HeaderMap, expected: &str) -> Result<(), StatusCode> {
    let supplied = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if supplied == Some(expected) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn digest(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value))
}
