use std::process::Command;

#[test]
fn startup_fails_fast_and_never_prints_secret_values() {
    let secret = "do-not-print-this-provider-secret";
    let output = Command::new(env!("CARGO_BIN_EXE_gavel-payments"))
        .env_clear()
        .env("DATABASE_URL", "postgres://invalid.example/payments")
        .env("AUTHZ_ENDPOINT", "http://authz.example")
        .env("PAYMENT_PROVIDER_ENDPOINT", "https://provider.example/")
        .env("PAYMENT_PROVIDER_SECRET", "short")
        .env("PAYMENT_CALLBACK_SECRET", secret)
        .env(
            "PAYMENT_FINGERPRINT_SECRET",
            "fingerprint-secret-at-least-32-bytes",
        )
        .env(
            "PROCESSES_SPIFFE_ID",
            "spiffe://cluster.local/ns/gavel/sa/gavel-processes",
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains(secret));
    assert!(!stderr.contains("short"));
    assert!(stderr.contains("secret configuration is invalid"));
}
