use std::{fs, path::Path};

fn rust_sources(root: &Path, output: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}

#[test]
fn application_core_has_no_transport_or_storage_imports() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/application");
    let mut sources = Vec::new();
    rust_sources(&root, &mut sources);
    assert!(!sources.is_empty());

    for source in sources {
        let body = fs::read_to_string(&source).unwrap();
        for forbidden in ["tonic::", "sqlx::", "axum::", "spicedb_rpc", "authzed::"] {
            assert!(
                !body.contains(forbidden),
                "{} imports outer-layer dependency {forbidden}",
                source.display()
            );
        }
    }
}
