extern crate prometheus;

use crate::custom_error::ErrorKind::BinVersion;
use crate::custom_error::{EngineInitError, ErrorKind};
use qovery_engine::cmd;
use qovery_engine::environment::models::types::DeployedEngineVersion;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::{env, fs, io};

pub fn check_libs_directory(path: String) -> Result<(), EngineInitError> {
    match fs::read_dir(path) {
        Ok(out) => {
            let is_empty = out.take(1).count() == 0;
            match is_empty {
                true => Err(EngineInitError::Regular(ErrorKind::LibsDirEmpty)),
                false => Ok(()),
            }
        }
        Err(_) => Err(EngineInitError::Regular(ErrorKind::LibsPathsMissing)),
    }
}

// check_versions_from will check (in file given in parameter) binaries versions
// will assert an error if used version installed is not the same as written in file
#[allow(dead_code)] // used by main for tests
pub fn check_versions_from(path: &str) -> Result<(), EngineInitError> {
    fn read_lines<P: AsRef<Path>>(filename: P) -> io::Result<io::Lines<BufReader<File>>> {
        let file = File::open(filename)?;
        Ok(BufReader::new(file).lines())
    }

    // please append this vector if you want to test more binaries
    let bin_to_check = ["terraform"];

    let lines: Vec<String> = read_lines(path)
        .map_err(|err| {
            error!("{}", err);
            EngineInitError::Regular(BinVersion)
        })?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|err| {
            error!("{}", err);
            EngineInitError::Regular(BinVersion)
        })?;

    // read line by line the version file
    for line in lines.iter() {
        // put in lowercase and split the BINARY_VERSION to BINARY
        let lowercase = line.to_lowercase();
        //TODO FIX Do not parse correctly binary names in bin_versions. It should split at = instead of _
        //Modify bin_version format and edit the parsing
        let binary_name = lowercase.split('_').next().unwrap_or("");

        // check if the binary need to be tested
        if bin_to_check.contains(&binary_name) {
            let result_cmd = cmd::command::run_version_command_for(binary_name);
            let version = lowercase.split('=').next_back().unwrap_or("").replace('"', "");

            if !result_cmd.contains(&version) {
                return Err(EngineInitError::Regular(BinVersion));
            }

            info!("{} is on right version {}", binary_name.to_string(), version);
        }
    }

    Ok(())
}

fn parse_deployed_engine_version(
    runtime_engine_version: Option<&str>,
    build_version_fallback: &str,
) -> DeployedEngineVersion {
    let raw_engine_version = runtime_engine_version
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(build_version_fallback);

    raw_engine_version
        .parse::<DeployedEngineVersion>()
        .expect("engine version must be a valid version or commit id")
}

/// Returns the deployed engine version from `ENGINE_TAG_VERSION` when it is set to a non-empty
/// value, otherwise falls back to the build-time version embedded in the binary.
pub fn load_deployed_engine_version(build_version_fallback: &str) -> DeployedEngineVersion {
    let runtime_engine_version = env::var("ENGINE_TAG_VERSION").ok();
    parse_deployed_engine_version(runtime_engine_version.as_deref(), build_version_fallback)
}

#[cfg(test)]
mod tests {
    use super::parse_deployed_engine_version;
    use qovery_engine::environment::models::types::DeployedEngineVersion;

    #[test]
    fn load_deployed_engine_version_prefers_runtime_env_var() {
        let deployed_engine_version = parse_deployed_engine_version(Some("v1.2.3"), "6b444021");

        assert_eq!(deployed_engine_version, "v1.2.3".parse::<DeployedEngineVersion>().unwrap());
    }

    #[test]
    fn load_deployed_engine_version_falls_back_to_build_version() {
        let deployed_engine_version = parse_deployed_engine_version(None, "6b444021");

        assert_eq!(deployed_engine_version, "6b444021".parse::<DeployedEngineVersion>().unwrap());
    }

    #[test]
    fn load_deployed_engine_version_falls_back_when_runtime_env_var_is_empty() {
        let deployed_engine_version = parse_deployed_engine_version(Some("   "), "6b444021");

        assert_eq!(deployed_engine_version, "6b444021".parse::<DeployedEngineVersion>().unwrap());
    }
}
