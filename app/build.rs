fn main() -> Result<(), Box<dyn std::error::Error>> {
    shadow_rs::new().unwrap();
    tonic_build::configure().compile(&["proto/engine.proto"], &[""])?;
    Ok(())
}
