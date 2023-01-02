fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("OUT_DIR", "src/grpc/");
    tonic_build::configure().compile(&["proto/engine.proto"], &[""])?;
    Ok(())
}
