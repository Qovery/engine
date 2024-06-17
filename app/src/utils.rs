extern crate prometheus;

use crate::custom_error::ErrorKind::BinVersion;
use crate::custom_error::{EngineInitError, ErrorKind};
use qovery_engine::cmd;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::{fs, io};

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
// will assert an error if used version installed is not not the same than written in file
pub fn check_versions_from(path: &str) -> Result<(), EngineInitError> {
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
            let version = lowercase.split('=').last().unwrap_or("").replace('"', "");

            if !result_cmd.contains(&version) {
                return Err(EngineInitError::Regular(BinVersion));
            }

            info!("{} is on right version {}", binary_name.to_string(), version);
        }
    }

    Ok(())
}

pub fn read_lines<P>(filename: P) -> io::Result<io::Lines<BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(BufReader::new(file).lines())
}

pub fn clean_configuration_directories() {
    // make sure kube config directory is cleaned
    // GKE gcloud command might set those eventually and eventually clashes later on
    info!("Deleting ~/.kube/config");
    if let Err(e) = fs::remove_dir_all("~/.kube/config") {
        warn!("Error while trying to delete ~/.kube/config, error: {}", e);
    }
}

#[cfg(test)]
mod test {
    use crate::utils::clean_configuration_directories;
    use std::fs::{create_dir_all, File};
    use std::io::Write;
    use std::path::Path;
    use uuid::Uuid;

    #[test]
    fn test_clean_configuration_directories() {
        // setup:

        // kubeconfigs
        let kubeconfig_file = format!("~/.kube/config/{}", Uuid::new_v4());
        let kubeconfig_path = Path::new(kubeconfig_file.as_str());
        if let Some(p) = kubeconfig_path.parent() {
            create_dir_all(p).expect("Cannot create directory")
        };
        let mut file = File::create(kubeconfig_path).expect("Cannot create file");
        file.write_all(b"Anything").expect("Cannot write file");
        assert!(kubeconfig_path.exists());

        // execute:
        clean_configuration_directories();

        // verify:
        assert!(!kubeconfig_path.exists());
    }
}
