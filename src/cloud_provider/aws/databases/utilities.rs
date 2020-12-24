use std::collections::HashMap;

use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cmd::kubectl::kubectl_exec_delete_secret;
use crate::error::{EngineError, StringError};
use crate::utilities::get_version_number;

pub fn delete_terraform_tfstate_secret(
    kubernetes: &dyn Kubernetes,
    secret_name: &str,
) -> Result<(), EngineError> {
    match kubernetes.config_file_path() {
        Ok(kube_config) => {
            //create the namespace to insert the tfstate in secrets
            let _ = kubectl_exec_delete_secret(
                kube_config,
                secret_name,
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables(),
            );

            Ok(())
        }
        Err(e) => {
            error!(
                "Failed to generate the kubernetes config file path: {:?}",
                e
            );

            Err(e)
        }
    }
}

pub fn get_supported_version_to_use<'a>(
    database_name: &str,
    all_supported_versions: HashMap<String, String>,
    version_to_check: &str,
) -> Result<String, StringError> {
    let version = match get_version_number(version_to_check) {
        Ok(version) => version,
        Err(e) => return Err(e),
    };

    // if a patch version is required
    if version.patch.is_some() {
        return match all_supported_versions.get(&format!(
            "{}.{}.{}",
            version.major,
            version.minor.unwrap().to_string(),
            version.patch.unwrap().to_string()
        )) {
            Some(version) => Ok(version.to_string()),
            None => {
                return Err(format!(
                    "{} {} version is not supported",
                    database_name, version_to_check
                ));
            }
        };
    }

    // if a minor version is required
    if version.minor.is_some() {
        return match all_supported_versions
            .get(&format!("{}.{}", version.major, version.minor.unwrap()).to_string())
        {
            Some(version) => Ok(version.to_string()),
            None => {
                return Err(format!(
                    "{} {} version is not supported",
                    database_name, version_to_check
                ));
            }
        };
    };

    // if only a major version is required
    match all_supported_versions.get(&version.major) {
        Some(version) => Ok(version.to_string()),
        None => {
            return Err(format!(
                "{} {} version is not supported",
                database_name, version_to_check
            ));
        }
    }
}

// Ease the support of multiple versions by range
pub fn generate_supported_version(
    major: i32,
    minor_min: i32,
    minor_max: i32,
    update_min: Option<i32>,
    update_max: Option<i32>,
    suffix_version: Option<String>,
) -> HashMap<String, String> {
    let mut supported_versions = HashMap::new();
    let mut latest_major_version = String::new();

    // blank suffix if not requested
    let suffix = match suffix_version {
        Some(suffix) => suffix,
        None => "".to_string(),
    };

    let _ = match update_min {
        // manage minor with updates
        Some(_) => {
            latest_major_version =
                format!("{}.{}.{}{}", major, minor_max, update_max.unwrap(), suffix);

            if minor_min == minor_max {
                // add short minor format targeting latest version
                supported_versions.insert(
                    format!("{}.{}", major.to_string(), minor_max.to_string()),
                    latest_major_version.clone(),
                );
                if update_min.unwrap() == update_max.unwrap() {
                    let version = format!("{}.{}.{}", major, minor_min, update_min.unwrap());
                    supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                } else {
                    for update in update_min.unwrap()..update_max.unwrap() + 1 {
                        let version = format!("{}.{}.{}", major, minor_min, update);
                        supported_versions
                            .insert(version.clone(), format!("{}{}", version, suffix));
                    }
                }
            } else {
                for minor in minor_min..minor_max + 1 {
                    // add short minor format targeting latest version
                    supported_versions.insert(
                        format!("{}.{}", major.to_string(), minor.to_string()),
                        format!(
                            "{}.{}.{}",
                            major.to_string(),
                            minor.to_string(),
                            update_max.unwrap().to_string()
                        ),
                    );
                    if update_min.unwrap() == update_max.unwrap() {
                        let version = format!("{}.{}.{}", major, minor, update_min.unwrap());
                        supported_versions
                            .insert(version.clone(), format!("{}{}", version, suffix));
                    } else {
                        for update in update_min.unwrap()..update_max.unwrap() + 1 {
                            let version = format!("{}.{}.{}", major, minor, update);
                            supported_versions
                                .insert(version.clone(), format!("{}{}", version, suffix));
                        }
                    }
                }
            }
        }
        // manage minor without updates
        None => {
            latest_major_version = format!("{}.{}{}", major, minor_max, suffix);
            for minor in minor_min..minor_max + 1 {
                let version = format!("{}.{}", major, minor);
                supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
            }
        }
    };

    // default major + major.minor supported version
    supported_versions.insert(major.to_string(), latest_major_version);

    supported_versions
}

pub fn get_tfstate_suffix(service_id: &str) -> String {
    return format!("{}", service_id.clone());
}

// Name generated from TF secret suffix
// https://www.terraform.io/docs/backends/types/kubernetes.html#secret_suffix
// As mention the doc: Secrets will be named in the format: tfstate-{workspace}-{secret_suffix}.
pub fn get_tfstate_name(service_id: &str) -> String {
    return format!("tfstate-default-{}", service_id);
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::databases::utilities::{get_tfstate_name, get_tfstate_suffix};

    #[test]
    fn check_tfstate_name() {
        assert_eq!(get_tfstate_name("randomid"), "tfstate-default-randomid");
        assert_eq!(get_tfstate_suffix("randomid"), "randomid");
    }
}
