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
fn sdk_lock_matches_exact_cargo_versions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lock: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("bsr-sdk.lock.json")).unwrap()).unwrap();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_eq!(lock["commit"], "ab5508e1a7a74c57b7c0c240607e9b88");
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
