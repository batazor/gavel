use std::{fs, path::Path};

const FORBIDDEN_DOMAIN_IMPORTS: &[&str] = &[
    "crate::application",
    "crate::adapters",
    "crate::bootstrap",
    "async_graphql",
    "axum::",
    "redis::",
    "sqlx::",
    "tokio::",
    "tonic::",
    "tracing::",
];

fn rust_files(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(path).expect("source directory must be readable") {
        let path = entry.expect("directory entry must be readable").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn domain_has_no_runtime_or_outer_layer_imports() {
    let domain = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domain");
    let mut files = Vec::new();
    rust_files(&domain, &mut files);

    let mut violations = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).expect("domain source must be readable");
        for forbidden in FORBIDDEN_DOMAIN_IMPORTS {
            if source.contains(forbidden) {
                violations.push(format!("{} imports {forbidden}", file.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "domain boundary violations: {violations:?}"
    );
}

#[test]
fn command_executor_has_no_cache_dependency() {
    let executor = include_str!("../src/application/executor.rs");
    for forbidden in ["AuctionCache", "PriceSnapshot", "redis"] {
        assert!(
            !executor.contains(forbidden),
            "command executor must not depend on cache symbol {forbidden}"
        );
    }
}
