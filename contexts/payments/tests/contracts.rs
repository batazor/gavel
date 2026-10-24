use std::{fs, path::Path};

#[test]
fn event_fixtures_are_versioned_and_delivery_safe() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixtures = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(fixtures.len(), 6);
    for path in fixtures {
        let source = fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&source).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert!(value["aggregateVersion"].as_u64().unwrap() > 0);
        for forbidden in [
            "providerRef",
            "customerRef",
            "instrumentRef",
            "recoveryUrl",
            "signature",
            "email",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} leaks {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn process_read_is_additive_and_recovery_data_stays_out_of_events() {
    let proto_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("proto")
        .join("gavel/payments/v1");
    let service = fs::read_to_string(proto_root.join("payments_service.proto")).unwrap();
    let events = fs::read_to_string(proto_root.join("payments_events.proto")).unwrap();

    for required in [
        "rpc GetSettlementAttempt",
        "string expected_auction_id",
        "uint32 expected_settlement_round",
        "string candidate_id",
        "SettlementRecoveryData recovery_required",
        "SettlementSuccessData succeeded",
    ] {
        assert!(
            service.contains(required),
            "missing process-read contract: {required}"
        );
    }
    assert!(service.contains("string recovery_url"));
    assert!(service.contains("string payment_id"));
    assert!(!events.contains("recovery_url"));
    assert!(!events.contains("recoveryUrl"));
}

#[test]
fn attempt_outbox_adapter_matches_the_registered_route() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let adapter = fs::read_to_string(root.join("src/adapters/outbound/postgres.rs")).unwrap();
    let migration = fs::read_to_string(root.join("migrations/0002_mvp_outbox_topic.sql")).unwrap();
    for required in [
        "gavel.payments.attempts.v1",
        "\"aggregateId\"",
        "\"aggregateVersion\"",
        "\"occurredAtMillis\"",
        "\"causationId\"",
    ] {
        assert!(
            adapter.contains(required),
            "missing outbox field: {required}"
        );
    }
    assert!(migration.contains("gavel.payments.attempts.v1"));
    for forbidden in ["recoveryUrl", "providerRef", "customerRef", "instrumentRef"] {
        assert!(!adapter.contains(forbidden), "outbox exposes {forbidden}");
    }
}

#[test]
fn sdk_lock_matches_exact_cargo_versions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lock: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("bsr-sdk.lock.json")).unwrap()).unwrap();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_eq!(lock["commit"], "ee7ab816c2f044159bc1e084ac557fda");
    assert_eq!(lock["label"], "implement-processes-temporal-workflows");
    for plugin in lock["generatedSdks"].as_object().unwrap().values() {
        let version = plugin["version"].as_str().unwrap();
        assert!(manifest.contains(&format!("version = \"={version}\"")));
    }
}

#[test]
fn graphql_schema_does_not_expose_provider_material() {
    let schema =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("graphql/schema.graphql"))
            .unwrap();
    for forbidden in ["customerRef", "instrumentRef", "providerRef", "signature"] {
        assert!(!schema.contains(forbidden));
    }
    for process_only in [
        "getSettlementAttempt",
        "expectedSettlementRound",
        "paymentId",
    ] {
        assert!(
            !schema.contains(process_only),
            "process-only field leaked into GraphQL: {process_only}"
        );
    }
}

#[test]
fn telemetry_uses_only_bounded_labels() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut source = String::new();
    for path in [
        root.join("main.rs"),
        root.join("adapters/inbound/callback.rs"),
        root.join("adapters/outbound/stripe.rs"),
        root.join("adapters/outbound/support.rs"),
    ] {
        source.push_str(&fs::read_to_string(path).unwrap());
    }
    for forbidden in [
        "auction_id\" =>",
        "attempt_id\" =>",
        "instrument_id\" =>",
        "principal_id\" =>",
        "idempotency_key\" =>",
    ] {
        assert!(!source.contains(forbidden));
    }
    for allowed in ["command\" =>", "outcome\" =>"] {
        assert!(source.contains(allowed));
    }
}

#[test]
fn runtime_bootstrap_supervises_all_routes_and_graceful_shutdown() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("main.rs"),
    )
    .unwrap();
    for route in [
        "\"/graphql\"",
        "\"/healthz\"",
        "\"/readyz\"",
        "\"/metrics\"",
        "callback_router(callback_ingress)",
        "sandbox_control_router(",
    ] {
        assert!(source.contains(route), "missing runtime route {route}");
    }
    for supervision in [
        "JoinSet",
        "CancellationToken",
        "with_graceful_shutdown",
        "serve_with_shutdown",
        "supervised runtime task",
        "SELECT 1",
        "PrometheusBuilder",
    ] {
        assert!(
            source.contains(supervision),
            "missing runtime supervision primitive {supervision}"
        );
    }
}
