use std::{fs, path::Path};

fn rust_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(rust_files(&path));
        } else if path.extension().and_then(|v| v.to_str()) == Some("rs") {
            result.push(path);
        }
    }
    result
}

#[test]
fn domain_and_application_do_not_import_framework_adapters() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for layer in ["domain", "application"] {
        for path in rust_files(&root.join(layer)) {
            let source = fs::read_to_string(&path).unwrap();
            for forbidden in [
                "sqlx::",
                "reqwest::",
                "tonic::",
                "async_graphql::",
                "Stripe",
                "Sandbox",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{} imports {forbidden}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn payments_and_processes_consume_exact_bsr_sdk_without_local_codegen() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!root.join("build.rs").exists());
    let payments = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let processes = fs::read_to_string(root.join("../processes/Cargo.toml")).unwrap();
    for manifest in [&payments, &processes] {
        assert!(manifest.contains("registry = \"buf\""));
        assert!(manifest.contains("version = \"=0.4.0-"));
        assert!(manifest.contains("version = \"=0.4.1-"));
        assert!(!manifest.contains("tonic-build"));
    }
    assert!(
        fs::read_to_string(root.join("src/lib.rs"))
            .unwrap()
            .contains("gavel_payments_prost")
    );
}
