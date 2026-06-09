use shadow_rs::SdResult;
use std::env;
use std::fs::File;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure().compile_protos(&["proto/engine.proto"], &[""])?;

    shadow_rs::ShadowBuilder::builder().hook(hook).build().unwrap();
    Ok(())
}

fn hook(file: &File) -> SdResult<()> {
    append_engine_version_from_env(file)?;
    Ok(())
}

fn append_engine_version_from_env(mut file: &File) -> SdResult<()> {
    let engine_version = env::var("ENGINE_TAG_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| env::var("CI_COMMIT_TAG").ok().filter(|value| !value.trim().is_empty()))
        .or_else(|| {
            env::var("CI_COMMIT_SHORT_SHA")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
    let engine_version = match engine_version {
        Some(engine_version) => format!(r#"pub const ENGINE_VERSION: &str = "{engine_version}";"#),
        None => r#"pub const ENGINE_VERSION: &str = SHORT_COMMIT;"#.to_string(),
    };
    let clippy_header = "#[allow(clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery)]";
    writeln!(file, "{clippy_header}\n{engine_version}")?;
    Ok(())
}
