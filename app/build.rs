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
    let ci_commit = env::var("CI_COMMIT_SHORT_SHA").unwrap_or_default();
    let engine_version: String = if !ci_commit.is_empty() {
        format!(r#"pub const ENGINE_VERSION: &str = "{ci_commit}";"#)
    } else {
        r#"pub const ENGINE_VERSION: &str = SHORT_COMMIT;"#.to_string()
    };
    let clippy_header = "#[allow(clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery)]";
    writeln!(file, "{clippy_header}\n{engine_version}")?;
    Ok(())
}
