use axum::{
    Json, Router,
    extract::Path,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use gavel_payments::{
    adapters::outbound::{StripeConfig, StripeGateway},
    application::{
        ChargeRequest, PortError, PrepareInstrumentRequest, ProviderGateway, ProviderOutcome,
    },
    domain::ProviderReference,
};
use serde_json::{Value, json};

async fn fake(
    Path(_path): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if body["idempotency_key"] != key {
        return Err(StatusCode::BAD_REQUEST);
    }
    if key == "timeout" {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let value = match key {
        "prepare" => {
            json!({"status":"prepared","session_ref":"session-1","url":"https://provider.example/register"})
        }
        "processing" => json!({"status":"processing","operation_ref":"operation-1"}),
        "recovery" => {
            json!({"status":"recovery_required","operation_ref":"operation-2","url":"https://provider.example/recover"})
        }
        "success" => json!({"status":"succeeded","operation_ref":"operation-3"}),
        "decline" => json!({"status":"declined"}),
        "malformed" => json!({"unexpected":true}),
        _ => json!({"status":"processing","operation_ref":"operation-timeout"}),
    };
    Ok(Json(value))
}

async fn gateway() -> StripeGateway {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/{*path}", post(fake)))
            .await
            .unwrap();
    });
    StripeGateway::new(
        reqwest::Client::new(),
        StripeConfig::new(
            url::Url::parse(&format!("http://{address}/")).unwrap(),
            "provider-secret-long".to_owned(),
            "callback-secret-long".to_owned(),
            300,
        )
        .unwrap(),
    )
}

fn charge(key: &str) -> ChargeRequest {
    ChargeRequest {
        provider_key: key.to_owned(),
        customer_ref: ProviderReference::parse("customer-1").unwrap(),
        instrument_ref: ProviderReference::parse("instrument-1").unwrap(),
        amount_minor_units: 100,
        currency_code: "USD".to_owned(),
        timeout_millis: if key == "timeout" { 10 } else { 1_000 },
    }
}

#[tokio::test]
async fn maps_all_provider_outcomes_and_malformed_or_timeout_failures() {
    let gateway = gateway().await;
    let prepared = gateway
        .prepare(PrepareInstrumentRequest {
            provider_key: "prepare".to_owned(),
            recovery_email: "owner@example.test".to_owned(),
            timeout_millis: 1_000,
        })
        .await
        .unwrap();
    assert_eq!(prepared.provider_url, "https://provider.example/register");
    assert!(matches!(
        gateway.charge(charge("processing")).await.unwrap(),
        ProviderOutcome::Processing { .. }
    ));
    assert!(matches!(
        gateway.charge(charge("recovery")).await.unwrap(),
        ProviderOutcome::RecoveryRequired { .. }
    ));
    assert!(matches!(
        gateway.charge(charge("success")).await.unwrap(),
        ProviderOutcome::Succeeded { .. }
    ));
    assert_eq!(
        gateway.charge(charge("decline")).await.unwrap(),
        ProviderOutcome::Declined
    );
    assert_eq!(
        gateway.charge(charge("malformed")).await.unwrap_err(),
        PortError::Internal
    );
    assert_eq!(
        gateway.charge(charge("timeout")).await.unwrap_err(),
        PortError::Timeout
    );
}
