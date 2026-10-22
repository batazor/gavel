use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use gavel_payments::adapters::{
    inbound::{CallbackFailure, CallbackIngress, CallbackSink, callback_router},
    outbound::{StripeConfig, StripeSignatureVerifier},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

struct Sink {
    bodies: Mutex<Vec<Vec<u8>>>,
    failure: Option<CallbackFailure>,
}
#[async_trait]
impl CallbackSink for Sink {
    async fn accept(&self, raw_body: &[u8]) -> Result<(), CallbackFailure> {
        self.bodies.lock().unwrap().push(raw_body.to_vec());
        self.failure.map_or(Ok(()), Err)
    }
}

fn signature(secret: &[u8], timestamp: u64, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    format!(
        "t={timestamp},v1={}",
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn ingress(sink: Arc<dyn CallbackSink>) -> Arc<CallbackIngress> {
    let config = StripeConfig::new(
        url::Url::parse("https://provider.example/").unwrap(),
        "provider-secret-long".to_owned(),
        "callback-secret-long".to_owned(),
        300,
    )
    .unwrap();
    Arc::new(CallbackIngress {
        verifier: StripeSignatureVerifier::new(&config),
        sink,
        now_secs: Arc::new(|| 1_000),
    })
}

#[tokio::test]
async fn preserves_raw_body_verifies_before_sink_and_returns_retryable_status() {
    let body = br#"{"id":"evt-1","nested":{"spacing":true}}"#;
    let sink = Arc::new(Sink {
        bodies: Mutex::new(vec![]),
        failure: None,
    });
    let app = callback_router(ingress(sink.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::post("/payment-provider-callbacks/stripe")
                .header(
                    "stripe-signature",
                    signature(b"callback-secret-long", 1_000, body),
                )
                .body(Body::from(body.as_slice()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(sink.bodies.lock().unwrap().as_slice(), &[body.to_vec()]);

    let response = app
        .oneshot(
            Request::post("/payment-provider-callbacks/stripe")
                .header("stripe-signature", "t=1000,v1=00")
                .body(Body::from(body.as_slice()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(sink.bodies.lock().unwrap().len(), 1);

    let retry = Arc::new(Sink {
        bodies: Mutex::new(vec![]),
        failure: Some(CallbackFailure::Retryable),
    });
    let response = callback_router(ingress(retry))
        .oneshot(
            Request::post("/payment-provider-callbacks/stripe")
                .header(
                    "stripe-signature",
                    signature(b"callback-secret-long", 1_000, body),
                )
                .body(Body::from(body.as_slice()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
