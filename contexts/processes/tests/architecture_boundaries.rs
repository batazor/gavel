use std::{fs, path::Path};

fn rust_sources(root: &Path) -> Vec<(String, String)> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push((
                    path.display().to_string(),
                    fs::read_to_string(path).unwrap(),
                ));
            }
        }
    }
    sources
}

#[test]
fn workflows_exclude_nondeterministic_and_transport_apis() {
    let workflow_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/workflows");
    let forbidden = [
        "tokio::",
        "tonic::",
        "reqwest::",
        "hyper::",
        "axum::",
        "std::io",
        "std::fs",
        "std::net",
        "std::thread",
        "SystemTime",
        "Instant::now",
        "rand::",
        "thread_rng",
        "temporalio_client",
    ];
    for (path, source) in rust_sources(&workflow_root) {
        for token in forbidden {
            assert!(
                !source.contains(token),
                "workflow source {path} uses forbidden API {token}"
            );
        }
    }
}

#[test]
fn workflow_dtos_are_framework_free_serializable_values() {
    let dto =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/workflows/dto.rs"))
            .unwrap();
    for framework in [
        "temporalio_",
        "prost::",
        "tonic::",
        "gavel_auction",
        "gavel_payments",
        "gavel_notify",
    ] {
        assert!(
            !dto.contains(framework),
            "workflow DTO depends on framework/owner SDK: {framework}"
        );
    }
    assert!(dto.contains("Serialize"));
    assert!(dto.contains("Deserialize"));
}

#[test]
fn processes_has_no_local_owner_contract_copies_or_codegen() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.ends_with("target") {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else {
                assert_ne!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("proto"),
                    "Processes must consume owner SDKs instead of copying {}",
                    path.display()
                );
            }
        }
    }
    assert!(!root.join("build.rs").exists());

    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    for owner_sdk in [
        "gavel-auction",
        "gavel-catalog",
        "gavel-notify",
        "gavel-payments-prost",
        "gavel-payments-tonic",
    ] {
        assert!(
            manifest.contains(owner_sdk),
            "missing owner SDK dependency {owner_sdk}"
        );
    }
    for local_codegen in ["prost-build", "tonic-build", "protoc-bin-vendored"] {
        assert!(
            !manifest.contains(local_codegen),
            "Processes must not compile owner contracts locally: {local_codegen}"
        );
    }
}
