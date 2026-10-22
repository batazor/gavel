fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    // SAFETY: Cargo runs this build script as a dedicated process before code generation.
    unsafe { std::env::set_var("PROTOC", protoc) };
    tonic_build::configure().compile_protos(
        &["proto/gavel/catalog/v1/catalog_service.proto"],
        &["proto"],
    )?;
    Ok(())
}
