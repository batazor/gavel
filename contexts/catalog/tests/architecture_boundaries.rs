use std::{fs, path::Path};

#[test]
fn domain_and_application_have_no_transport_storage_or_sdk_imports() {
    for root in ["src/domain", "src/application"] {
        inspect(Path::new(root));
    }
}

fn inspect(path: &Path) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            inspect(&path);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            let source = fs::read_to_string(&path).unwrap();
            for forbidden in [
                "tonic::",
                "sqlx::",
                "async_graphql::",
                "reqwest::",
                "aws_sdk_",
                "crate::rpc",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{} imports forbidden outer dependency {forbidden}",
                    path.display()
                );
            }
        }
    }
}
