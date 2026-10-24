use crate::{
    adapters::outbound::{PostgresNotifications, SmtpEmailGateway},
    application::{ApplicationError, NotificationView, NotifyApplication},
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;

#[derive(Clone)]
pub struct HttpState {
    pub repository: Arc<PostgresNotifications>,
    pub smtp: Arc<SmtpEmailGateway>,
    pub metrics: PrometheusHandle,
    pub application: Arc<NotifyApplication>,
    pub local_operator_token: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct NotificationQuery {
    notification_id: String,
}
pub async fn health() -> StatusCode {
    StatusCode::OK
}
pub async fn ready(State(state): State<HttpState>) -> StatusCode {
    let db = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(state.repository.pool())
        .await
        .is_ok();
    if db && state.smtp.ready().await {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
pub async fn metrics(State(state): State<HttpState>) -> impl IntoResponse {
    (StatusCode::OK, state.metrics.render())
}

/// A local-only, token-guarded projection used by the kind acceptance harness. It deliberately
/// exposes neither rendered message data nor provider references.
pub async fn local_notification(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(query): Query<NotificationQuery>,
) -> Result<Json<NotificationView>, StatusCode> {
    let expected = state
        .local_operator_token
        .as_deref()
        .ok_or(StatusCode::NOT_FOUND)?;
    authorize(&headers, expected)?;
    if query.notification_id.trim().is_empty() || query.notification_id.len() > 160 {
        return Err(StatusCode::BAD_REQUEST);
    }
    state
        .application
        .get(&query.notification_id)
        .await
        .map_err(|error| match error {
            ApplicationError::DependencyUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
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
