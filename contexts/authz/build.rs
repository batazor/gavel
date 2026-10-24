fn main() -> Result<(), Box<dyn std::error::Error>> {
    // sqlx::migrate! embeds the migration directory in the binary. Make changes
    // to migration files invalidate Cargo's otherwise reusable build output.
    println!("cargo:rerun-if-changed=migrations");

    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    // SAFETY: Cargo runs this build script as a dedicated process before code generation.
    unsafe { std::env::set_var("PROTOC", protoc) };
    let descriptor_path =
        std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("authz_descriptor.bin");
    tonic_build::configure()
        .file_descriptor_set_path(descriptor_path)
        .compile_protos(
            &[
                "proto/gavel/authz/v1/authz_service.proto",
                "proto/authzed/api/v1/permission_service.proto",
            ],
            &["proto"],
        )?;
    Ok(())
}
