use crate::cmd::helm::HelmError::CmdError;
use crate::cmd::helm::{Helm, HelmCommand, HelmError};
use crate::cmd::kubectl::{
    kubectl_apply_with_path, kubectl_create_secret_from_file, kubectl_delete_secret, kubectl_exec_get_secrets,
    kubectl_get_resource_yaml, kubernetes_is_metrics_server_working,
};
use crate::cmd::structs::HelmChartVersions;
use crate::errors::CommandError;
use crate::fs::{
    create_yaml_backup_file, create_yaml_file_from_secret, indent_file, remove_lines_starting_with,
    truncate_file_from_word,
};
use crate::helm::ChartInfo;
use retry::delay::Fixed;
use retry::OperationResult;
use semver::Version;
use serde_derive::Deserialize;
use std::fs::OpenOptions;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Backup {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BackupInfos {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChartDependencyYAML {
    #[serde(default)]
    pub repository: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChartYAML {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub dependencies: Vec<ChartDependencyYAML>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BackupStatus {
    pub is_backupable: bool,
    pub backup_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CRDSUpdate {
    pub path: String,
    pub resources: Vec<String>,
}

pub fn prepare_chart_backup<P, T>(
    kubernetes_config: P,
    workspace_root_dir: T,
    chart: &ChartInfo,
    envs: &[(&str, &str)],
    backup_resources: Vec<String>,
) -> Result<Vec<BackupInfos>, HelmError>
where
    P: AsRef<Path>,
    T: AsRef<Path>,
{
    let mut backups: Vec<Backup> = vec![];
    for backup_resource in backup_resources {
        match kubectl_get_resource_yaml(
            &kubernetes_config,
            envs.to_vec(),
            backup_resource.as_str(),
            Some(chart.get_namespace_string().as_str()),
        ) {
            Ok(content) => {
                if !content.to_lowercase().contains("no resources found") {
                    backups.push(Backup {
                        name: backup_resource,
                        content,
                    });
                };
            }
            Err(e) => {
                error!("Kubectl error: {:?}", e.message_safe())
            }
        };
    }

    let mut backup_infos: Vec<BackupInfos> = vec![];

    if backups.is_empty() {
        return Ok(backup_infos);
    }

    for backup in backups.clone() {
        if !backup.content.is_empty() && !backup.content.contains("items: []") {
            match create_yaml_backup_file(
                workspace_root_dir.as_ref(),
                chart.name.to_string(),
                Some(backup.name.clone()),
                backup.content,
            ) {
                Ok(path) => {
                    backup_infos.push(BackupInfos {
                        name: backup.name,
                        path,
                    });
                }
                Err(e) => return Err(CmdError(chart.name.clone(), HelmCommand::UPGRADE, e)),
            }
        }
    }

    for backup_info in backup_infos.clone() {
        if let Err(e) = remove_lines_starting_with(
            backup_info.path.clone(),
            vec!["resourceVersion", "uid", "apiVersion: v1", "items", "kind: List"],
        ) {
            return Err(CmdError(
                chart.name.clone(),
                HelmCommand::UPGRADE,
                CommandError::new(
                    format!("Error while editing YAML backup file {}.", backup_info.name),
                    Some(e.to_string()),
                    None,
                ),
            ));
        }

        if let Err(e) = truncate_file_from_word(backup_info.path.clone(), "metadata") {
            return Err(CmdError(
                chart.name.clone(),
                HelmCommand::UPGRADE,
                CommandError::new(
                    format!("Error while editing YAML backup file {}.", backup_info.name),
                    Some(e.to_string()),
                    None,
                ),
            ));
        }

        if let Err(e) = indent_file(backup_info.path.clone()) {
            return Err(CmdError(
                chart.name.clone(),
                HelmCommand::UPGRADE,
                CommandError::new(
                    format!("Error while editing YAML backup file {}.", backup_info.name),
                    Some(e.to_string()),
                    None,
                ),
            ));
        }

        let backup_name = format!("{}-{}-q-backup", chart.name, backup_info.name);
        if let Err(e) = kubectl_create_secret_from_file(
            &kubernetes_config,
            envs.to_vec(),
            Some(chart.namespace.to_string().as_str()),
            backup_name,
            backup_info.name,
            backup_info.path,
        ) {
            return Err(CmdError(
                chart.name.clone(),
                HelmCommand::UPGRADE,
                CommandError::new(e.message_safe(), e.message_raw(), None),
            ));
        }
    }

    Ok(backup_infos)
}

pub fn apply_chart_backup<P>(
    kubernetes_config: P,
    workspace_root_dir: P,
    envs: &[(&str, &str)],
    chart: &ChartInfo,
) -> Result<(), HelmError>
where
    P: AsRef<Path>,
{
    let secrets = kubectl_exec_get_secrets(
        &kubernetes_config,
        chart.clone().namespace.to_string().as_str(),
        "",
        envs.to_vec(),
    )
    .map_err(|e| {
        CmdError(
            chart.clone().name,
            HelmCommand::UPGRADE,
            CommandError::new(e.message_safe(), e.message_raw(), None),
        )
    })?
    .items;

    for secret in secrets {
        if secret.metadata.name.contains("-q-backup") {
            let path = match create_yaml_file_from_secret(&workspace_root_dir, secret.clone()) {
                Ok(path) => path,
                Err(e) => match e.message_safe().to_lowercase().contains("no content") {
                    true => match kubectl_delete_secret(
                        &kubernetes_config,
                        envs.to_vec(),
                        Some(chart.clone().namespace.to_string().as_str()),
                        secret.metadata.name,
                    ) {
                        Ok(_) => continue,
                        Err(e) => {
                            return Err(CmdError(
                                chart.clone().name,
                                HelmCommand::UPGRADE,
                                CommandError::new(e.message_safe(), e.message_raw(), None),
                            ))
                        }
                    },
                    false => {
                        return Err(CmdError(
                            chart.clone().name,
                            HelmCommand::UPGRADE,
                            CommandError::new(e.message_safe(), e.message_raw(), None),
                        ))
                    }
                },
            };

            if let Err(e) = kubectl_apply_with_path(&kubernetes_config, envs.to_vec(), path.as_str(), None) {
                return Err(CmdError(
                    chart.clone().name,
                    HelmCommand::UPGRADE,
                    CommandError::new(e.message_safe(), e.message_raw(), None),
                ));
            };

            if let Err(e) = kubectl_delete_secret(
                &kubernetes_config,
                envs.to_vec(),
                Some(chart.clone().namespace.to_string().as_str()),
                secret.metadata.name,
            ) {
                return Err(CmdError(
                    chart.clone().name,
                    HelmCommand::UPGRADE,
                    CommandError::new(e.message_safe(), e.message_raw(), None),
                ));
            };
        }
    }

    Ok(())
}

pub fn delete_unused_chart_backup<P>(
    kubernetes_config: P,
    envs: &[(&str, &str)],
    chart: &ChartInfo,
) -> Result<(), HelmError>
where
    P: AsRef<Path>,
{
    let secrets = kubectl_exec_get_secrets(
        &kubernetes_config,
        chart.clone().namespace.to_string().as_str(),
        "",
        envs.to_vec(),
    )
    .map_err(|e| {
        CmdError(
            chart.clone().name,
            HelmCommand::UPGRADE,
            CommandError::new(e.message_safe(), e.message_raw(), None),
        )
    })?
    .items;

    for secret in secrets {
        if secret.metadata.name.contains("-q-backup") {
            if let Err(e) = kubectl_delete_secret(
                &kubernetes_config,
                envs.to_vec(),
                Some(chart.clone().namespace.to_string().as_str()),
                secret.metadata.name,
            ) {
                return Err(CmdError(
                    chart.clone().name,
                    HelmCommand::UPGRADE,
                    CommandError::new(e.message_safe(), e.message_raw(), None),
                ));
            };
        }
    }

    Ok(())
}

pub fn get_common_helm_chart_infos(chart: &ChartInfo) -> Result<ChartYAML, HelmError> {
    let string_path = format!("{}/Chart.yaml", chart.path);
    let file = OpenOptions::new().read(true).open(string_path.as_str()).map_err(|e| {
        CmdError(
            chart.clone().name,
            HelmCommand::UPGRADE,
            CommandError::new(
                format!("Unable to get chart infos for {}.", chart.name.clone()),
                Some(e.to_string()),
                None,
            ),
        )
    })?;
    let mut content = String::new();
    let _ = BufReader::new(file).read_to_string(&mut content);
    match serde_yaml::from_str::<ChartYAML>(content.as_str()) {
        Ok(chart_yaml) => Ok(chart_yaml),
        Err(e) => Err(CmdError(
            chart.clone().name,
            HelmCommand::UPGRADE,
            CommandError::new(
                format!("Unable to get chart infos for {}.", chart.name.clone()),
                Some(e.to_string()),
                None,
            ),
        )),
    }
}

pub fn get_common_helm_chart_version(chart: &ChartInfo) -> Result<Option<Version>, HelmError> {
    let chart_yaml = match get_common_helm_chart_infos(chart) {
        Ok(chart_yaml) => chart_yaml,
        Err(e) => {
            return Err(CmdError(
                chart.clone().name,
                HelmCommand::UPGRADE,
                CommandError::new(
                    format!("Unable to get chart version for {}.", chart.name.clone()),
                    Some(e.to_string()),
                    None,
                ),
            ))
        }
    };

    if !chart_yaml.version.is_empty() {
        let mut chart_version = chart_yaml.version;
        if chart_version.starts_with('v') {
            chart_version = chart_version[1..].to_string();
        }
        return match Version::parse(chart_version.as_str()) {
            Ok(version) => Ok(Some(version)),
            Err(e) => Err(CmdError(
                chart.clone().name,
                HelmCommand::UPGRADE,
                CommandError::new(
                    format!("Unable to get chart version for {}.", chart.name.clone()),
                    Some(e.to_string()),
                    None,
                ),
            )),
        };
    }

    Err(CmdError(
        chart.clone().name,
        HelmCommand::UPGRADE,
        CommandError::new_from_safe_message(format!("Unable to get chart version for {}.", chart.name.clone())),
    ))
}

pub fn prepare_chart_backup_on_upgrade<P>(
    kubernetes_config: P,
    chart: ChartInfo,
    envs: &[(&str, &str)],
    installed_version: Option<Version>,
) -> Result<BackupStatus, HelmError>
where
    P: AsRef<Path>,
{
    let mut need_backup = false;
    let root_dir_path = std::env::temp_dir();

    if chart.backup_resources.is_some() && installed_version.le(&get_common_helm_chart_version(&chart)?) {
        prepare_chart_backup(
            kubernetes_config,
            root_dir_path.as_path(),
            &chart,
            envs,
            chart.backup_resources.as_ref().unwrap().to_vec(),
        )?;
        need_backup = true;
    }

    Ok(BackupStatus {
        is_backupable: need_backup,
        backup_path: root_dir_path,
    })
}

fn common_chart_check<P>(
    chart_name: &str,
    interval: u64,
    retries: usize,
    check: Result<P, CommandError>,
) -> Result<(), CommandError> {
    let result = retry::retry(Fixed::from(Duration::from_secs(interval)).take(retries), || match &check {
        Ok(_) => OperationResult::Ok(()),
        Err(err) => OperationResult::Retry(format!("command error: {err}")),
    });

    let error_message_safe = format!(
        "{} is not ready after {} min retries",
        chart_name,
        interval * (retries as u64) / 60
    );
    match result {
        Err(retry::Error { error, .. }) => Err(CommandError::new(error_message_safe, Some(error), None)),
        Ok(_) => Ok(()),
    }
}

pub fn verify_metrics_server<P>(kubernetes_config: P, envs: &[(String, String)]) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    common_chart_check(
        "Metrics Server",
        10,
        30,
        kubernetes_is_metrics_server_working(
            kubernetes_config,
            envs.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect(),
        ),
    )
}

pub fn update_crds_on_upgrade<P>(
    kubernetes_config: P,
    chart: ChartInfo,
    envs: &[(&str, &str)],
    helm: &Helm,
) -> Result<(), HelmError>
where
    P: AsRef<Path>,
{
    let crd_update = match chart.crds_update {
        Some(crd_update) => crd_update,
        None => return Ok(()),
    };

    // Get chart version that is currently installed
    let installed_versions = helm.get_chart_version(&chart.name, Some(chart.namespace.to_string().as_str()), envs)?;
    let app_version = match installed_versions {
        Some(HelmChartVersions {
            app_version: Some(version),
            ..
        }) => version,
        _ => return Ok(()),
    };

    // ok very flaky to do it like that :x
    // check that the CRDS is correctly installed with the correct version
    if crd_update
        .path
        .contains(&format!("/v{}.{}.", app_version.major, app_version.minor))
    {
        return Ok(());
    }

    for crd in crd_update.resources {
        if let Err(e) = kubectl_apply_with_path(
            &kubernetes_config,
            envs.to_vec(),
            &format!("{}/{}", crd_update.path, crd),
            Some(vec!["--server-side", "--force-conflicts"]),
        ) {
            return Err(CmdError(chart.name, HelmCommand::UPGRADE, e));
        }
    }

    Ok(())
}

impl ChartDependencyYAML {
    pub fn is_https(&self) -> bool {
        self.repository.starts_with("https://")
    }

    pub fn is_oci(&self) -> bool {
        self.repository.starts_with("oci://")
    }
}
