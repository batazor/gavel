use gavel_payments::adapters::outbound::{StripeConfig, StripeSignatureVerifier};
use hmac::{Hmac, Mac};
use sha2::Sha256;

fn header(secret: &[u8], timestamp: u64, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let signature = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("t={timestamp},v1={signature}")
}

#[test]
fn verifies_raw_body_and_rejects_tampering_or_stale_timestamp() {
    let secret = "callback-secret-at-least-32-bytes";
    let config = StripeConfig::new(
        url::Url::parse("https://provider.example/").unwrap(),
        "provider-secret-long".to_owned(),
        secret.to_owned(),
        300,
    )
    .unwrap();
    let verifier = StripeSignatureVerifier::new(&config);
    let body = br#"{"id":"evt_1"}"#;
    assert!(
        verifier
            .verify(&header(secret.as_bytes(), 1_000, body), body, 1_001)
            .is_ok()
    );
    assert!(
        verifier
            .verify(&header(secret.as_bytes(), 1_000, body), b"{}", 1_001)
            .is_err()
    );
    assert!(
        verifier
            .verify(&header(secret.as_bytes(), 1_000, body), body, 1_400)
            .is_err()
    );
    assert!(!format!("{config:?}").contains(secret));
}
