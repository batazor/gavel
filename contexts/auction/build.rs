fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    // SAFETY: Cargo runs this build script as a dedicated process before code generation.
    unsafe { std::env::set_var("PROTOC", protoc) };
    let descriptor_path =
        std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("auction_descriptor.bin");
    tonic_build::configure()
        .file_descriptor_set_path(descriptor_path)
        .compile_protos(
            &[
                "proto/gavel/auction/v1/auction_service.proto",
                "proto/gavel/auction/v1/auction_events.proto",
                "../authz/proto/gavel/authz/v1/authz_service.proto",
            ],
            &["proto", "../authz/proto"],
        )?;
    Ok(())
}
