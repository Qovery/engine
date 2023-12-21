use std::io::{Error, Write};
use std::path::{Path, PathBuf};

use tracing::{error, info};

use crate::cloud_provider::helm::ChartInfo;
use crate::cmd::command::{CommandError, CommandKiller, ExecutableCommand, QoveryCommand};
use crate::cmd::helm::HelmCommand::{FETCH, LIST, LOGIN, PULL, ROLLBACK, STATUS, UNINSTALL, UPGRADE};
use crate::cmd::helm::HelmError::{
    CannotRollback, CmdError, InvalidKubeConfig, InvalidRepositoryConfig, ReleaseDoesNotExist,
};
use crate::cmd::structs::{HelmChart, HelmChartVersions, HelmListItem};
use crate::errors;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::io_models::container::Registry;
use semver::Version;
use serde_derive::Deserialize;
use std::fs::File;
use std::str::FromStr;
use tempfile::TempDir;
use url::Url;

const HELM_DEFAULT_TIMEOUT_IN_SECONDS: u32 = 600;
const HELM_MAX_HISTORY: &str = "50";

pub enum Timeout<T> {
    Default,
    Value(T),
}

impl Timeout<u32> {
    pub fn value(&self) -> u32 {
        match *self {
            Timeout::Default => HELM_DEFAULT_TIMEOUT_IN_SECONDS,
            Timeout::Value(t) => t,
        }
    }
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum HelmError {
    #[error("Kubernetes config file path is not valid or does not exist: {0}")]
    InvalidKubeConfig(PathBuf),

    #[error("Requested Helm release `{0}` does not exist")]
    ReleaseDoesNotExist(String),

    #[error("Requested Helm release `{0}` is under an helm lock. Ensure release is de-locked before going further")]
    ReleaseLocked(String),

    #[error("Helm release `{0}` during helm {1:?} has been rollbacked")]
    Rollbacked(String, HelmCommand),

    #[error("Helm release `{0}` cannot be rollbacked due to be at revision 1")]
    CannotRollback(String),

    #[error("Helm timed out for release `{0}` during helm {1:?}: {2}")]
    Timeout(String, HelmCommand, String),

    #[error("Command killed by user request: {0}")]
    Killed(String, HelmCommand),

    #[error("Helm command `{1:?}` for release {0} terminated with an error: {2:?}")]
    CmdError(String, HelmCommand, errors::CommandError),

    #[error("Invalid Helm Repository Config: {0}")]
    InvalidRepositoryConfig(String),
}

#[derive(Debug)]
pub struct Helm {
    kubernetes_config: PathBuf,
    common_envs: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
pub enum HelmCommand {
    ROLLBACK,
    STATUS,
    UPGRADE,
    UNINSTALL,
    LIST,
    DIFF,
    TEMPLATE,
    FETCH,
    PULL,
    LOGIN,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReleaseInfo {
    // https://github.com/helm/helm/blob/12f1bc0acdeb675a8c50a78462ed3917fb7b2e37/pkg/release/status.go
    status: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReleaseStatus {
    pub version: u64,
    pub info: ReleaseInfo,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChartVersion {
    pub app_version: u64,
    pub version: ReleaseInfo,
}

impl ReleaseStatus {
    fn is_locked(&self) -> bool {
        self.info.status.starts_with("pending-")
    }
}

impl Helm {
    fn get_all_envs<'a>(&'a self, envs: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all_envs: Vec<(&str, &str)> = self.common_envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all_envs.append(&mut envs.to_vec());

        all_envs
    }

    pub fn new<P: AsRef<Path>>(kubernetes_config: P, common_envs: &[(&str, &str)]) -> Result<Helm, HelmError> {
        // Check kube config file is valid
        let kubernetes_config = kubernetes_config.as_ref().to_path_buf();
        if !kubernetes_config.exists() || !kubernetes_config.is_file() {
            return Err(InvalidKubeConfig(kubernetes_config));
        }

        Ok(Helm {
            kubernetes_config,
            common_envs: common_envs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        })
    }

    pub fn check_release_exist(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<ReleaseStatus, HelmError> {
        let namespace = chart.get_namespace_string();
        let args = vec![
            "status",
            &chart.name,
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "--namespace",
            &namespace,
            "-o",
            "json",
        ];

        let mut stdout = String::new();
        let mut stderr = String::new();
        match helm_exec_with_output(
            &args,
            &self.get_all_envs(envs),
            &mut |line| stdout.push_str(&line),
            &mut |line| stderr.push_str(&line),
            &CommandKiller::never(),
        ) {
            Err(_) if stderr.contains("release: not found") => Err(ReleaseDoesNotExist(chart.name.clone())),
            Err(err) => {
                stderr.push_str(err.to_string().as_str());
                Err(CmdError(chart.name.clone(), STATUS, err.into()))
            }
            Ok(_) => {
                let status: ReleaseStatus = serde_json::from_str(&stdout).unwrap_or_default();
                Ok(status)
            }
        }
    }

    pub fn rollback(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        if self.check_release_exist(chart, envs)?.version <= 1 {
            return Err(CannotRollback(chart.name.clone()));
        }

        let timeout = format!("{}s", &chart.timeout_in_seconds);
        let namespace = chart.get_namespace_string();
        let args = vec![
            "rollback",
            &chart.name,
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "--namespace",
            &namespace,
            "--timeout",
            &timeout,
            "--history-max",
            HELM_MAX_HISTORY,
            "--cleanup-on-fail",
            "--force",
            "--wait",
        ];

        let mut stderr = String::new();
        match helm_exec_with_output(
            &args,
            &self.get_all_envs(envs),
            &mut |_| {},
            &mut |line| stderr.push_str(&line),
            &CommandKiller::never(),
        ) {
            Err(err) => {
                stderr.push_str(err.to_string().as_str());
                Err(CmdError(chart.name.clone(), ROLLBACK, err.into()))
            }
            Ok(_) => Ok(()),
        }
    }

    pub fn uninstall<STDOUT, STDERR>(
        &self,
        chart: &ChartInfo,
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
        stdout_output: &mut STDOUT,
        stderr_output: &mut STDERR,
    ) -> Result<(), HelmError>
    where
        STDOUT: FnMut(String),
        STDERR: FnMut(String),
    {
        // If the release does not exist, we do not return an error
        match self.check_release_exist(chart, envs) {
            Ok(_) => {}
            Err(ReleaseDoesNotExist(_)) => return Ok(()),
            Err(err) => return Err(err),
        }

        let timeout = format!("{}s", &chart.timeout_in_seconds);
        let namespace = chart.get_namespace_string();
        let args = vec![
            "uninstall",
            &chart.name,
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "--namespace",
            &namespace,
            "--timeout",
            &timeout,
            "--wait",
            "--cascade=foreground",
            "--debug",
        ];

        let mut stderr = String::new();
        match helm_exec_with_output(
            &args,
            &self.get_all_envs(envs),
            stdout_output,
            &mut |line| {
                stderr.push_str(&line);
                stderr_output(line)
            },
            cmd_killer,
        ) {
            Err(err) => {
                stderr.push_str(&err.to_string());
                Err(CmdError(chart.name.clone(), UNINSTALL, err.into()))
            }
            Ok(_) => Ok(()),
        }
    }

    fn unlock_release(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        match self.check_release_exist(chart, envs) {
            Ok(release) if release.is_locked() && release.version <= 1 => {
                info!("Helm lock detected. Uninstalling it as it is the first version and rollback is not possible");
                self.uninstall(chart, envs, &CommandKiller::never(), &mut |_| {}, &mut |_| {})?;
            }
            Ok(release) if release.is_locked() => {
                info!("Helm lock detected. Forcing rollback to previous version");
                self.rollback(chart, envs)?;
            }
            Ok(release) => {
                // Happy path nothing to do
                debug!("Helm release status: {:?}", release)
            }
            Err(_) => {} // Happy path nothing to do
        }

        Ok(())
    }

    /// List deployed helm charts
    ///
    /// # Arguments
    ///
    /// * `envs` - environment variables required for kubernetes connection
    /// * `namespace` - list charts from a kubernetes namespace or use None to select all namespaces
    pub fn list_release(&self, namespace: Option<&str>, envs: &[(&str, &str)]) -> Result<Vec<HelmChart>, HelmError> {
        let mut helm_args = vec![
            "list",
            "-a",
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "-o",
            "json",
        ];
        match namespace {
            Some(ns) => helm_args.append(&mut vec!["-n", ns]),
            None => helm_args.push("-A"),
        }

        let mut output_string: Vec<String> = Vec::with_capacity(20);
        if let Err(cmd_error) = helm_exec_with_output(
            &helm_args,
            &self.get_all_envs(envs),
            &mut |line| output_string.push(line),
            &mut |line| error!("{}", line),
            &CommandKiller::never(),
        ) {
            return Err(CmdError("none".to_string(), LIST, cmd_error.into()));
        }

        let values = serde_json::from_str::<Vec<HelmListItem>>(&output_string.join(""));
        let mut helms_charts: Vec<HelmChart> = Vec::new();

        match values {
            Ok(all_helms) => {
                for helm in all_helms {
                    // chart version is stored in chart name (i.e loki-3.4.5) so we look for last dash position to parse name.
                    let mut last_dash_pos = helm.chart.rfind('-').expect("Can't parse helm chart") + 1;
                    // sometime chart version in name start with 'v' (i.e loki-v3.4.5). We squeeze it.
                    if helm.chart[last_dash_pos..].starts_with('v') {
                        last_dash_pos += 1
                    }

                    let chart_version_raw = helm.chart[last_dash_pos..].to_string();
                    let chart_version = Version::from_str(chart_version_raw.as_str()).ok();

                    let mut app_version_raw = helm.app_version;
                    // sometime app version start with 'v'. We squeeze it.
                    if app_version_raw.starts_with('v') {
                        app_version_raw = app_version_raw[1..].to_string()
                    }
                    let app_version = Version::from_str(app_version_raw.as_str()).ok();

                    helms_charts.push(HelmChart::new(helm.name, helm.namespace, chart_version, app_version))
                }

                Ok(helms_charts)
            }
            Err(e) => Err(CmdError(
                "none".to_string(),
                LIST,
                errors::CommandError::new(
                    "Error while deserializing all helms names".to_string(),
                    Some(e.to_string()),
                    Some(
                        envs.iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect::<Vec<(String, String)>>(),
                    ),
                ),
            )),
        }
    }

    pub fn get_chart_version(
        &self,
        chart_name: &str,
        namespace: Option<&str>,
        envs: &[(&str, &str)],
    ) -> Result<Option<HelmChartVersions>, HelmError> {
        let deployed_charts = self.list_release(namespace, envs)?;
        for chart in deployed_charts {
            if chart.name == chart_name {
                return Ok(Some(HelmChartVersions {
                    chart_version: chart.chart_version,
                    app_version: chart.app_version,
                }));
            }
        }

        // found nothing ;'(
        Ok(None)
    }

    pub fn download_chart(
        &self,
        repository: &Url,
        engine_helm_registry: &Registry,
        chart_name: &str,
        chart_version: &str,
        target_directory: &Path,
        skip_tls_verification: bool,
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
    ) -> Result<(), HelmError> {
        return match repository.scheme() {
            "https" => self.download_https_chart(
                repository,
                chart_name,
                chart_version,
                target_directory,
                skip_tls_verification,
                envs,
                cmd_killer,
            ),
            "oci" => self.download_oci_chart(
                engine_helm_registry,
                chart_name,
                chart_version,
                target_directory,
                skip_tls_verification,
                envs,
                cmd_killer,
            ),
            _ => Err(InvalidRepositoryConfig(format!(
                "Invalid repository scheme {}",
                repository.scheme()
            ))),
        };
    }

    pub fn download_oci_chart(
        &self,
        engine_helm_registry: &Registry,
        chart_name: &str,
        chart_version: &str,
        target_directory: &Path,
        skip_tls_verification: bool,
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
    ) -> Result<(), HelmError> {
        // Try to use the parent directory to create a tmp dir, because later moving directory
        // does work across mount point. In test, on our laptop, /tmp is always on a separate mount point using tmpfs
        // So use same target dir, to avoid issues
        let tmpdir = Self::get_temp_dir(target_directory, chart_name, PULL)?;

        let url_with_credentials = engine_helm_registry.get_url_with_credentials();
        if let Some((registry_url, username, password)) =
            Self::get_registry_with_username_password(&url_with_credentials)
        {
            let envs = self.get_all_envs(envs);
            let mut helm_registry =
                HelmRegistry::new(&registry_url, &username, &password, tmpdir.path(), &envs, cmd_killer);

            // Will be logout when HelmRegistry will be dropped.
            helm_registry.login(skip_tls_verification)?;

            self.helm_pull(
                engine_helm_registry,
                chart_name,
                chart_version,
                &envs,
                cmd_killer,
                &tmpdir,
                &url_with_credentials,
                skip_tls_verification,
            )?;
        } else {
            self.helm_pull(
                engine_helm_registry,
                chart_name,
                chart_version,
                envs,
                cmd_killer,
                &tmpdir,
                &url_with_credentials,
                skip_tls_verification,
            )?;
        }

        // Move the chart from tmpdir to the target_directory of the user
        // Rename must not cross mount point boundaries. It is ok as we don't have a tmpfs inside our container and we use user provided target_dir
        let name = chart_name.split('/').last().unwrap_or_default();
        std::fs::rename(tmpdir.path().join(name), target_directory).map_err(|err| {
            CmdError(
                chart_name.to_string(),
                PULL,
                errors::CommandError::new(
                    format!(
                        "Cannot move chart folder out of the tmpdir from {:?} to {:?}",
                        tmpdir.path().join(chart_name),
                        target_directory
                    ),
                    Some(err.to_string()),
                    Some(vec![]),
                ),
            )
        })?;

        Ok(())
    }

    fn helm_pull(
        &self,
        engine_helm_registry: &Registry,
        chart_name: &str,
        chart_version: &str,
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
        tmpdir: &TempDir,
        url_with_credentials: &Url,
        skip_tls_verification: bool,
    ) -> Result<(), HelmError> {
        let url_with_chart_name = match engine_helm_registry.get_url().join(chart_name) {
            Ok(url_with_chart_name) => url_with_chart_name,
            Err(_) => {
                error!("Can't join chart_name to registry url");
                return Err(InvalidRepositoryConfig("Can't join chart_name to registry url".to_string()));
            }
        };

        let (registry_config_path, repository_config_path, repository_cache_path) =
            Self::get_helm_cmd_paths(tmpdir.path());
        let mut helm_pull_args = vec![
            "pull",
            "--debug", // there is no debug log but if someday they appear
            url_with_chart_name.as_str(),
            "--version",
            chart_version,
            "--untar",
            "--untardir",
            tmpdir.path().to_str().unwrap_or_default(),
            "--registry-config",
            &registry_config_path,
            "--repository-config",
            &repository_config_path,
            "--repository-cache",
            &repository_cache_path,
        ];

        if skip_tls_verification {
            helm_pull_args.push("--insecure-skip-tls-verify");
        }

        let mut error_message: Vec<String> = Vec::new();
        let helm_ret = helm_exec_with_output(
            helm_pull_args.as_slice(),
            envs,
            &mut |line| {
                info!("{}", line);
            },
            &mut |line| {
                warn!("chart {}: {}", chart_name, line);
                // we don't want to flood user with debug log
                if line.contains(" [debug] ") {
                    return;
                }
                error_message.push(line);
            },
            cmd_killer,
        );

        if let Err(err) = helm_ret {
            error!("Helm error: {:?}", err);

            // Try do define/specify a bit more the message
            let stderr_msg: String = error_message.into_iter().collect();
            let stderr_msg = format!("{stderr_msg}: {err}");

            // If the helm command has been canceled by the user, propagate correctly the killed error
            return match err {
                CommandError::TimeoutError(_) => Err(HelmError::Timeout(chart_name.to_string(), PULL, stderr_msg)),
                CommandError::Killed(_) => Err(HelmError::Killed(chart_name.to_string(), PULL)),
                _ => Err(CmdError(
                    chart_name.to_string(),
                    PULL,
                    errors::CommandError::new(
                        format!(
                            "Helm failed to pull chart {} at version {} from {}",
                            chart_name,
                            chart_version,
                            url_with_credentials.as_str()
                        ),
                        Some(stderr_msg),
                        Some(envs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
                    ),
                )),
            };
        };
        Ok(())
    }

    fn get_helm_cmd_paths(helm_cmd_path: &Path) -> (String, String, String) {
        let registry_config_path = helm_cmd_path.join("config.json").display().to_string();
        let repository_config_path = helm_cmd_path.join("repositories.yaml").display().to_string();
        let repository_cache_path = helm_cmd_path.display().to_string();
        (registry_config_path, repository_config_path, repository_cache_path)
    }

    pub fn download_https_chart(
        &self,
        repository: &Url,
        chart_name: &str,
        chart_version: &str,
        target_directory: &Path,
        skip_tls_verification: bool,
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
    ) -> Result<(), HelmError> {
        // Try to use the parent directory to create a tmp dir, because later moving directory
        // does work across mount point. In test, on our laptop, /tmp is always on a separate mount point using tmpfs
        // So use same target dir, to avoid issues
        let tmpdir = Self::get_temp_dir(target_directory, chart_name, FETCH)?;

        let mut helm_args = vec![
            "fetch",
            "--debug", // there is no debug log but if someday they appear
            "--repo",
            repository.as_str(),
            chart_name,
            "--version",
            chart_version,
            "--untar",
            "--untardir",
            tmpdir.path().to_str().unwrap_or_default(),
        ];

        if skip_tls_verification {
            helm_args.push("--insecure-skip-tls-verify");
        }

        let login = urlencoding::decode(repository.username()).unwrap_or_default();
        let password = repository
            .password()
            .map(|password| urlencoding::decode(password).unwrap_or_default());

        if let Some(password) = &password {
            helm_args.extend_from_slice(&["--pass-credentials", "--username", &login, "--password", password])
        }

        let mut error_message: Vec<String> = Vec::new();
        let helm_ret = helm_exec_with_output(
            helm_args.as_slice(),
            &self.get_all_envs(envs),
            &mut |line| {
                info!("{}", line);
            },
            &mut |line| {
                warn!("chart {}: {}", chart_name, line);
                // we don't want to flood user with debug log
                if line.contains(" [debug] ") {
                    return;
                }
                error_message.push(line);
            },
            cmd_killer,
        );

        if let Err(err) = helm_ret {
            error!("Helm error: {:?}", err);

            // Try do define/specify a bit more the message
            let stderr_msg: String = error_message.into_iter().collect();
            let stderr_msg = format!("{stderr_msg}: {err}");

            // If the helm command has been canceled by the user, propagate correctly the killed error
            return match err {
                CommandError::TimeoutError(_) => Err(HelmError::Timeout(chart_name.to_string(), FETCH, stderr_msg)),
                CommandError::Killed(_) => Err(HelmError::Killed(chart_name.to_string(), FETCH)),
                _ => Err(CmdError(
                    chart_name.to_string(),
                    FETCH,
                    errors::CommandError::new(
                        format!(
                            "Helm failed to fetch chart {} at version {} from {}",
                            chart_name, chart_version, repository
                        ),
                        Some(stderr_msg),
                        Some(envs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
                    ),
                )),
            };
        };

        // Move the chart from tmpdir to the target_directory of the user
        // Rename must not cross mount point boundaries. It is ok as we don't have a tmpfs inside our container and we use user provided target_dir
        std::fs::rename(tmpdir.path().join(chart_name), target_directory).map_err(|err| {
            CmdError(
                chart_name.to_string(),
                FETCH,
                errors::CommandError::new(
                    format!(
                        "Cannot move chart folder out of the tmpdir from {:?} to {:?}",
                        tmpdir.path().join(chart_name),
                        target_directory
                    ),
                    Some(err.to_string()),
                    Some(vec![]),
                ),
            )
        })?;

        Ok(())
    }

    fn get_temp_dir(
        target_directory: &Path,
        chart_name: &str,
        helm_command: HelmCommand,
    ) -> Result<TempDir, HelmError> {
        if !target_directory.is_dir() {
            return Err(CmdError(
                chart_name.to_string(),
                helm_command,
                errors::CommandError::new(
                    "Target directory where to download the chart does not exist or is not a directory".to_string(),
                    None,
                    Some(vec![]),
                ),
            ));
        }

        let tmpdir = target_directory
            .parent()
            .map(tempfile::tempdir_in)
            .unwrap_or_else(tempfile::tempdir)
            .map_err(|err| {
                CmdError(
                    chart_name.to_string(),
                    helm_command,
                    errors::CommandError::new(
                        "Cannot create tmp dir to fetch chart".to_string(),
                        Some(err.to_string()),
                        Some(vec![]),
                    ),
                )
            })?;
        Ok(tmpdir)
    }

    fn get_registry_with_username_password(url_with_credentials: &Url) -> Option<(String, String, String)> {
        let registry_url = match url_with_credentials.host_str() {
            Some(registry_url) => registry_url.to_string(),
            None => {
                warn!("can't get the host of the registry url");
                return None;
            }
        };

        let username = match urlencoding::decode(url_with_credentials.username()) {
            Ok(decoded_username) => decoded_username.to_string(),
            Err(_) => {
                warn!("can't get the username of the registry");
                return None;
            }
        };

        let password = match url_with_credentials
            .password()
            .and_then(|password| urlencoding::decode(password).ok())
        {
            Some(password) => password.to_string(),
            None => {
                warn!("can't get the password of the registry");
                return None;
            }
        };

        Some((registry_url, username, password))
    }

    pub fn upgrade_diff(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        let mut args_string: Vec<String> = vec![
            "diff".to_string(),
            "upgrade".to_string(),
            "--kubeconfig".to_string(),
            self.kubernetes_config.to_str().unwrap_or_default().to_string(),
            "--install".to_string(),
            "--namespace".to_string(),
            chart.get_namespace_string(),
        ];

        for value in &chart.values {
            args_string.push("--set".to_string());
            args_string.push(format!("{}={}", value.key, value.value));
        }

        for value_file in &chart.values_files {
            args_string.push("-f".to_string());
            args_string.push(value_file.clone());
        }

        for value_file in &chart.yaml_files_content {
            let file_path = format!("{}/{}", chart.path, &value_file.filename);
            let file_create = || -> Result<(), Error> {
                let mut file = File::create(&file_path)?;
                file.write_all(value_file.yaml_content.as_bytes())?;
                Ok(())
            };

            // no need to validate yaml as it will be done by helm
            if let Err(e) = file_create() {
                let cmd_err = errors::CommandError::new(
                    format!("Error while writing yaml content to file `{}`", &file_path),
                    Some(format!("Content\n{}\nError: {}", value_file.yaml_content, e)),
                    Some(
                        envs.iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect::<Vec<(String, String)>>(),
                    ),
                );
                return Err(CmdError(chart.name.clone(), UPGRADE, cmd_err));
            };

            args_string.push("-f".to_string());
            args_string.push(file_path);
        }

        // add last elements
        args_string.push(chart.name.clone());
        args_string.push(chart.path.clone());

        let mut stderr_msg = String::new();
        let helm_ret = helm_exec_with_output(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(envs),
            &mut |line| {
                info!("{}", line);
            },
            &mut |line| {
                stderr_msg.push_str(&line);
                warn!("chart {}: {}", chart.name, line);
            },
            &CommandKiller::never(),
        );

        match helm_ret {
            // Ok is ok
            Ok(_) => Ok(()),
            Err(err) => {
                error!("Helm error: {:?}", err);
                Err(CmdError(chart.name.clone(), HelmCommand::DIFF, err.into()))
            }
        }
    }

    pub fn upgrade(
        &self,
        chart: &ChartInfo,
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
    ) -> Result<(), HelmError> {
        // Due to crash or error it is possible that the release is under an helm lock
        // Try to un-stuck the situation first if needed
        // We don't care if the rollback failed, as it is a best effort to remove the lock
        // and to re-launch an upgrade just after
        let unlock_ret = self.unlock_release(chart, envs);
        info!("Helm lock status: {:?}", unlock_ret);

        let timeout_string = format!("{}s", &chart.timeout_in_seconds);

        let mut args_string: Vec<String> = vec![
            "upgrade".to_string(),
            "--kubeconfig".to_string(),
            self.kubernetes_config.to_str().unwrap_or_default().to_string(),
            "--create-namespace".to_string(),
            "--cleanup-on-fail".to_string(),
            "--install".to_string(),
            "--debug".to_string(),
            "--timeout".to_string(),
            timeout_string.as_str().to_string(),
            "--history-max".to_string(),
            HELM_MAX_HISTORY.to_string(),
            "--namespace".to_string(),
            chart.get_namespace_string(),
        ];

        // warn: don't add debug or json output won't work
        if chart.atomic {
            args_string.push("--atomic".to_string())
        }
        if chart.force_upgrade {
            args_string.push("--force".to_string())
        }
        if chart.recreate_pods {
            args_string.push("--recreate-pods".to_string())
        }
        if chart.dry_run {
            args_string.push("--dry-run".to_string())
        }
        if chart.wait {
            args_string.push("--wait".to_string())
        }

        // overrides and files overrides
        for value in &chart.values {
            args_string.push("--set".to_string());
            args_string.push(format!("{}={}", value.key, value.value));
        }
        for value in &chart.values_string {
            args_string.push("--set-string".to_string());
            args_string.push(format!("{}={}", value.key, value.value));
        }

        for value_file in &chart.values_files {
            args_string.push("-f".to_string());
            args_string.push(value_file.clone());
        }
        for value_file in &chart.yaml_files_content {
            let file_path = format!("{}/{}", chart.path, &value_file.filename);
            let file_create = || -> Result<(), Error> {
                let mut file = File::create(&file_path)?;
                file.write_all(value_file.yaml_content.as_bytes())?;
                Ok(())
            };

            // no need to validate yaml as it will be done by helm
            if let Err(e) = file_create() {
                let cmd_err = errors::CommandError::new(
                    format!("Error while writing yaml content to file `{}`", &file_path),
                    Some(format!("Content\n{}\nError: {}", value_file.yaml_content, e)),
                    Some(
                        envs.iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect::<Vec<(String, String)>>(),
                    ),
                );
                return Err(CmdError(chart.name.clone(), UPGRADE, cmd_err));
            };

            args_string.push("-f".to_string());
            args_string.push(file_path);
        }

        // add last elements
        args_string.push(chart.name.clone());
        args_string.push(chart.path.clone());

        let mut error_message: Vec<String> = vec![];

        let helm_ret = helm_exec_with_output(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(envs),
            &mut |line| {
                info!("chart {}: {}", chart.name, line);
            },
            &mut |line| {
                warn!("chart {}: {}", chart.name, line);
                // we don't want to flood user with debug log
                if line.contains(" [debug] ") {
                    return;
                }
                error_message.push(line);
            },
            cmd_killer,
        );

        if let Err(err) = helm_ret {
            error!("Helm error: {:?}", err);

            // Try do define/specify a bit more the message
            let stderr_msg: String = error_message.into_iter().collect();
            let stderr_msg = format!("{stderr_msg}: {err}",);

            // If the helm command has been canceled by the user, propagate correctly the killed error
            match err {
                CommandError::TimeoutError(_) => {
                    return Err(HelmError::Timeout(chart.name.clone(), UPGRADE, stderr_msg));
                }
                CommandError::Killed(_) => {
                    return Err(HelmError::Killed(chart.name.clone(), UPGRADE));
                }
                _ => {}
            }

            let error = if stderr_msg.contains("another operation (install/upgrade/rollback) is in progress") {
                HelmError::ReleaseLocked(chart.name.clone())
            } else if stderr_msg.contains("has been rolled back") {
                HelmError::Rollbacked(chart.name.clone(), UPGRADE)
            } else if stderr_msg.contains("timed out waiting") || stderr_msg.contains("deadline exceeded") {
                HelmError::Timeout(chart.name.clone(), UPGRADE, stderr_msg)
            } else {
                CmdError(
                    chart.name.clone(),
                    UPGRADE,
                    errors::CommandError::new(
                        "Helm upgrade error".to_string(),
                        Some(stderr_msg),
                        Some(envs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
                    ),
                )
            };

            return Err(error);
        };

        Ok(())
    }

    pub fn uninstall_chart_if_breaking_version(
        &self,
        chart: &ChartInfo,
        envs: &[(&str, &str)],
    ) -> Result<(), HelmError> {
        // If there is a breaking version set for the current helm chart,
        // then we compare this breaking version with the currently installed version if any.
        // If current installed version is older than breaking change one, then we delete
        // the chart before applying it.
        if let Some(breaking_version) = &chart.reinstall_chart_if_installed_version_is_below_than {
            if let Some(installed_versions) =
                self.get_chart_version(&chart.name, Some(chart.get_namespace_string().as_str()), envs)?
            {
                if let Some(version) = installed_versions.chart_version {
                    if &version < breaking_version {
                        self.uninstall(chart, envs, &CommandKiller::never(), &mut |_| {}, &mut |_| {})?;
                    }
                }
            }
        }

        Ok(())
    }

    // Used by helmchart service to validate deployed resources
    pub fn template_raw<STDERR>(
        &self,
        release_name: &str,
        chart_path: &Path,
        namespace: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
        stderr_output: &mut STDERR,
    ) -> Result<String, HelmError>
    where
        STDERR: FnMut(String),
    {
        let chart_path = chart_path.to_string_lossy();
        let args: Vec<&str> = [
            "template",
            release_name,
            chart_path.as_ref(),
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "-n",
            namespace,
        ]
        .into_iter()
        .chain(args.iter().copied())
        .collect();

        let mut stdout = String::new();
        let mut stderr_msg = String::new();
        let helm_ret = helm_exec_with_output(
            &args,
            &self.get_all_envs(envs),
            &mut |line| {
                stdout.push_str(&line);
                stdout.push('\n');
            },
            &mut |line| {
                stderr_msg.push_str(&line);
                warn!("chart {}: {}", release_name, line);
                stderr_output(line);
            },
            cmd_killer,
        );

        match helm_ret {
            // Ok is ok
            Ok(_) => Ok(stdout),
            Err(err) => match err {
                CommandError::TimeoutError(_) => Err(HelmError::Timeout(release_name.to_string(), UPGRADE, stderr_msg)),
                CommandError::Killed(_) => Err(HelmError::Killed(release_name.to_string(), UPGRADE)),
                _ => {
                    error!("Helm error: {:?}", err);
                    Err(CmdError(release_name.to_string(), HelmCommand::TEMPLATE, err.into()))
                }
            },
        }
    }

    // Used by helmchart service that takes its argument raw, as it users that control them
    pub fn upgrade_raw<STDOUT, STDERR>(
        &self,
        release_name: &str,
        chart_path: &Path,
        namespace: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
        stdout_output: &mut STDOUT,
        stderr_output: &mut STDERR,
    ) -> Result<(), HelmError>
    where
        STDOUT: FnMut(String),
        STDERR: FnMut(String),
    {
        // Due to crash or error it is possible that the release is under an helm lock
        // Try to un-stuck the situation first if needed
        // We don't care if the rollback failed, as it is a best effort to remove the lock
        // and to re-launch an upgrade just after
        let chart = ChartInfo::new_from_release_name(release_name, namespace);
        let unlock_ret = self.unlock_release(&chart, envs);
        info!("Helm lock status: {:?}", unlock_ret);

        let chart_path = chart_path.to_string_lossy();
        let args: Vec<&str> = [
            "upgrade",
            release_name,
            chart_path.as_ref(),
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "--install",
            "-n",
            namespace,
        ]
        .into_iter()
        .chain(args.iter().copied())
        .collect();

        let mut stderr_msgs: String = String::new();
        let helm_ret = helm_exec_with_output(
            &args,
            &self.get_all_envs(envs),
            &mut |line| {
                info!("chart {}: {}", chart.name, line);
                stdout_output(line);
            },
            &mut |line| {
                warn!("chart {}: {}", chart.name, line);
                stderr_msgs.push_str(&line);
                stderr_output(line);
            },
            cmd_killer,
        );

        if let Err(err) = helm_ret {
            error!("Helm error: {:?}", err);

            // If the helm command has been canceled by the user, propagate correctly the killed error
            match err {
                CommandError::TimeoutError(_) => {
                    return Err(HelmError::Timeout(chart.name.clone(), UPGRADE, err.to_string()));
                }
                CommandError::Killed(_) => {
                    return Err(HelmError::Killed(chart.name.clone(), UPGRADE));
                }
                _ => {}
            }

            let error = if stderr_msgs.contains("another operation (install/upgrade/rollback) is in progress") {
                HelmError::ReleaseLocked(chart.name.clone())
            } else if stderr_msgs.contains("has been rolled back") {
                HelmError::Rollbacked(chart.name.clone(), UPGRADE)
            } else if stderr_msgs.contains("timed out waiting") || stderr_msgs.contains("deadline exceeded") {
                HelmError::Timeout(chart.name.clone(), UPGRADE, err.to_string())
            } else {
                CmdError(
                    chart.name.clone(),
                    UPGRADE,
                    errors::CommandError::new(
                        "Helm upgrade error".to_string(),
                        Some(err.to_string()),
                        Some(envs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
                    ),
                )
            };

            return Err(error);
        };

        Ok(())
    }

    pub fn template_validate(
        &self,
        chart: &ChartInfo,
        envs: &[(&str, &str)],
        output_render_directory: Option<&str>,
    ) -> Result<(), HelmError> {
        let mut args_string: Vec<String> = vec![
            "template".to_string(),
            "--validate".to_string(),
            "--debug".to_string(),
            "--kubeconfig".to_string(),
            self.kubernetes_config.to_str().unwrap_or_default().to_string(),
            "--namespace".to_string(),
            chart.get_namespace_string(),
        ];

        if let Some(output_dir) = output_render_directory {
            args_string.push("--output-dir".to_string());
            args_string.push(output_dir.to_string());
        }

        for value in &chart.values {
            args_string.push("--set".to_string());
            args_string.push(format!("{}={}", value.key, value.value));
        }
        for value in &chart.values_string {
            args_string.push("--set-string".to_string());
            args_string.push(format!("{}={}", value.key, value.value));
        }

        for value_file in &chart.values_files {
            args_string.push("-f".to_string());
            args_string.push(value_file.clone());
        }

        for value_file in &chart.yaml_files_content {
            let file_path = format!("{}/{}", chart.path, &value_file.filename);
            let file_create = || -> Result<(), Error> {
                let mut file = File::create(&file_path)?;
                file.write_all(value_file.yaml_content.as_bytes())?;
                Ok(())
            };

            // no need to validate yaml as it will be done by helm
            if let Err(e) = file_create() {
                let cmd_err = errors::CommandError::new(
                    format!("Error while writing yaml content to file `{}`", &file_path),
                    Some(format!("Content\n{}\nError: {}", value_file.yaml_content, e)),
                    Some(
                        envs.iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect::<Vec<(String, String)>>(),
                    ),
                );
                return Err(CmdError(chart.name.clone(), UPGRADE, cmd_err));
            };

            args_string.push("-f".to_string());
            args_string.push(file_path);
        }

        // add last elements
        args_string.push(chart.name.clone());
        args_string.push(chart.path.clone());

        let mut stderr_msg = String::new();
        let helm_ret = helm_exec_with_output(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(envs),
            &mut |line| {
                debug!("{}", line);
            },
            &mut |line| {
                stderr_msg.push_str(&line);
                warn!("chart {}: {}", chart.name, line);
            },
            &CommandKiller::never(),
        );

        match helm_ret {
            // Ok is ok
            Ok(_) => Ok(()),
            Err(err) => {
                error!("Helm error: {:?}", err);
                Err(CmdError(chart.name.clone(), HelmCommand::TEMPLATE, err.into()))
            }
        }
    }
}

fn helm_exec_with_output<STDOUT, STDERR>(
    args: &[&str],
    envs: &[(&str, &str)],
    stdout_output: &mut STDOUT,
    stderr_output: &mut STDERR,
    cmd_killer: &CommandKiller,
) -> Result<(), CommandError>
where
    STDOUT: FnMut(String),
    STDERR: FnMut(String),
{
    // Note: Helm CLI use spf13/cobra lib for the CLI; One function is mainly used to return an error if a command failed.
    // Helm returns an error each time a command does not succeed as they want. Which leads to handling error with status code 1
    // It means that the command successfully ran, but it didn't terminate as expected
    let mut cmd = QoveryCommand::new("helm", args, envs);
    match cmd.exec_with_abort(stdout_output, stderr_output, cmd_killer) {
        Err(err) => Err(err),
        _ => Ok(()),
    }
}

pub fn to_engine_error(event_details: &EventDetails, error: HelmError) -> EngineError {
    EngineError::new_helm_error(event_details.clone(), error)
}

struct HelmRegistry<'a> {
    registry_url: &'a str,
    username: &'a str,
    password: &'a str,
    repository_cache_path: &'a Path,
    envs: &'a [(&'a str, &'a str)],
    cmd_killer: &'a CommandKiller<'a>,
    login: bool,
}

impl<'a> HelmRegistry<'a> {
    fn new(
        registry_url: &'a str,
        username: &'a str,
        password: &'a str,
        repository_cache_path: &'a Path,
        envs: &'a [(&'a str, &'a str)],
        cmd_killer: &'a CommandKiller,
    ) -> Self {
        HelmRegistry {
            registry_url,
            username,
            password,
            repository_cache_path,
            envs,
            cmd_killer,
            login: false,
        }
    }

    fn login(&mut self, skip_tls_verification: bool) -> Result<(), HelmError> {
        let (registry_config_path, repository_config_path, repository_cache_path) =
            Helm::get_helm_cmd_paths(self.repository_cache_path);
        let mut helm_login_args = vec![
            "registry",
            "--debug", // there is no debug log but if someday they appear
            "login",
            self.registry_url,
            "--username",
            self.username,
            "--password",
            self.password,
            "--registry-config",
            &registry_config_path,
            "--repository-config",
            &repository_config_path,
            "--repository-cache",
            &repository_cache_path,
        ];

        if skip_tls_verification {
            helm_login_args.push("--insecure");
        }

        let mut error_message: Vec<String> = Vec::new();
        let helm_ret = helm_exec_with_output(
            helm_login_args.as_slice(),
            self.envs,
            &mut |line| {
                info!("{}", line);
            },
            &mut |line| {
                warn!("repository {}: {}", self.registry_url, self.username);
                // we don't want to flood user with debug log
                if line.contains(" [debug] ") {
                    return;
                }
                error_message.push(line);
            },
            self.cmd_killer,
        );

        if let Err(err) = helm_ret {
            error!("Helm error: {:?}", err);

            // Try do define/specify a bit more the message
            let stderr_msg: String = error_message.into_iter().collect();
            let stderr_msg = format!("{stderr_msg}: {err}");

            // If the helm command has been canceled by the user, propagate correctly the killed error
            return match err {
                CommandError::TimeoutError(_) => {
                    Err(HelmError::Timeout(self.registry_url.to_string(), LOGIN, stderr_msg))
                }
                CommandError::Killed(_) => Err(HelmError::Killed(self.registry_url.to_string(), LOGIN)),
                _ => Err(CmdError(
                    self.registry_url.to_string(),
                    LOGIN,
                    errors::CommandError::new(
                        format!(
                            "Helm failed to login repository {} for username {}",
                            self.registry_url, self.username,
                        ),
                        Some(stderr_msg),
                        Some(self.envs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
                    ),
                )),
            };
        };

        self.login = true;
        Ok(())
    }
}

impl Drop for HelmRegistry<'_> {
    fn drop(&mut self) {
        if !self.login {
            return;
        }

        let (registry_config_path, repository_config_path, repository_cache_path) =
            Helm::get_helm_cmd_paths(self.repository_cache_path);
        let helm_logout_args = vec![
            "registry",
            "--debug", // there is no debug log but if someday they appear
            "logout",
            self.registry_url,
            "--registry-config",
            &registry_config_path,
            "--repository-config",
            &repository_config_path,
            "--repository-cache",
            &repository_cache_path,
        ];

        let mut error_message: Vec<String> = Vec::new();
        let helm_ret = helm_exec_with_output(
            helm_logout_args.as_slice(),
            self.envs,
            &mut |line| {
                info!("{}", line);
            },
            &mut |line| {
                warn!("repository {}: {}", self.registry_url, self.username);
                // we don't want to flood user with debug log
                if line.contains(" [debug] ") {
                    return;
                }
                error_message.push(line);
            },
            self.cmd_killer,
        );
        if let Err(err) = helm_ret {
            error!("Helm logout error: {:?}", err);
        };
    }
}

#[cfg(feature = "test-local-kube")]
#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::{ChartInfo, ChartSetValue};
    use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
    use crate::cmd::helm::{helm_exec_with_output, Helm, HelmError};
    use crate::deployment_action::deploy_helm::default_helm_timeout;
    use crate::io_models::container::Registry::GenericCr;
    use semver::Version;
    use std::path::Path;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;
    use url::Url;
    use uuid::Uuid;

    struct HelmTestCtx {
        helm: Helm,
        charts: Vec<ChartInfo>,
    }

    impl HelmTestCtx {
        fn cleanup(&self) {
            for chart in &self.charts {
                let ret = self
                    .helm
                    .uninstall(chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {});
                assert!(ret.is_ok())
            }
        }

        fn new(release_name: &str) -> HelmTestCtx {
            let charts = vec![ChartInfo::new_from_custom_namespace(
                release_name.to_string(),
                "tests/helm/simple_nginx".to_string(),
                "default".to_string(),
                default_helm_timeout().as_secs() as i64,
                vec![],
                vec![],
                vec![],
                false,
                None,
            )];
            let mut kube_config = dirs::home_dir().unwrap();
            kube_config.push(".kube/config");
            let helm = Helm::new(kube_config.to_str().unwrap(), &[]).unwrap();

            let cleanup = HelmTestCtx { helm, charts };
            cleanup.cleanup();
            cleanup
        }
    }

    impl Drop for HelmTestCtx {
        fn drop(&mut self) {
            self.cleanup()
        }
    }

    #[test]
    fn check_version() {
        let mut output = String::new();
        let _ = helm_exec_with_output(
            &["version"],
            &[],
            &mut |line| output.push_str(&line),
            &mut |_line| {},
            &CommandKiller::never(),
        );
        assert!(output.contains("Version:\"v3.12.3\""));
    }

    #[test]
    fn test_release_exist() {
        let HelmTestCtx { ref helm, ref charts } = HelmTestCtx::new("test-release-exist");
        let ret = helm.check_release_exist(&charts[0], &[]);

        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name))
    }

    #[test]
    fn test_list_release() {
        let HelmTestCtx {
            ref helm,
            ref mut charts,
        } = HelmTestCtx::new("test-list-release");
        charts[0].custom_namespace = Some("hello-my-friend-this-is-a-test".to_string());

        // no existing namespace should return an empty array
        let ret = helm.list_release(Some("tsdfsfsdf"), &[]);
        assert!(matches!(ret, Ok(vec) if vec.is_empty()));

        // install something
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        // We should have at least one release in all the release
        let ret = helm.list_release(None, &[]);
        assert!(matches!(ret, Ok(vec) if !vec.is_empty()));

        // We should have at least one release in all the release
        let ret = helm.list_release(Some(&charts[0].get_namespace_string()), &[]);
        assert!(matches!(ret, Ok(vec) if vec.len() == 1));

        // Install a second stuff
        let HelmTestCtx {
            ref helm,
            ref mut charts,
        } = HelmTestCtx::new("test-list-release-2");
        charts[0].custom_namespace = Some("hello-my-friend-this-is-a-test".to_string());
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        let ret = helm.list_release(Some(&charts[0].get_namespace_string()), &[]);
        assert!(matches!(ret, Ok(vec) if vec.len() == 2));
    }

    #[test]
    fn test_upgrade_diff() {
        let HelmTestCtx { ref helm, ref charts } = HelmTestCtx::new("test-upgrade-diff");

        let ret = helm.upgrade_diff(&charts[0], &[]);
        assert!(matches!(ret, Ok(())));
    }

    #[test]
    fn test_rollback() {
        let HelmTestCtx { ref helm, ref charts } = HelmTestCtx::new("test-rollback");

        // check release does not exist yet
        let ret = helm.rollback(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name));

        // install it
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        // First revision cannot be rollback
        let ret = helm.rollback(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::CannotRollback(_))));

        // 2nd upgrade
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        // Rollback should be ok now
        let ret = helm.rollback(&charts[0], &[]);
        assert!(matches!(ret, Ok(())));
    }

    #[test]
    fn test_upgrade() {
        let HelmTestCtx { ref helm, ref charts } = HelmTestCtx::new("test-upgrade");

        // check release does not exist yet
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name));

        // install it
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        // check now it exists
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(ret.is_ok());
    }

    #[test]
    fn test_upgrade_timeout() {
        let HelmTestCtx {
            ref helm,
            ref mut charts,
        } = HelmTestCtx::new("test-upgrade-timeout");
        charts[0].timeout_in_seconds = 1;

        // check release does not exist yet
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name));

        // install it
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Err(HelmError::Timeout(_, _, _))));

        // Release should not exist if it fails
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name));
    }

    #[test]
    fn test_upgrade_with_lock_during_install() {
        // We want to check that we manage to install a chart even if a lock is present while it was the first installation
        let HelmTestCtx {
            ref helm,
            ref mut charts,
        } = HelmTestCtx::new("test-upgrade-with-lock-install");

        // check release does not exist yet
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name));

        // Spawn our task killer
        let barrier = Arc::new(Barrier::new(2));
        thread::spawn({
            let barrier = barrier.clone();
            let chart_name = charts[0].name.clone();
            move || {
                barrier.wait();
                thread::sleep(Duration::from_millis(5000));
                let mut cmd = QoveryCommand::new("pkill", &["-9", "-f", &format!("helm.*{chart_name}")], &[]);
                let _ = cmd.exec();
            }
        });

        // install it
        charts[0].values = vec![ChartSetValue {
            key: "initialDelaySeconds".to_string(),
            value: "10".to_string(),
        }];
        barrier.wait();
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(ret.is_err());

        // Release should be locked
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Ok(release) if release.is_locked()));

        // New installation should work even if a lock is present
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        // Release should not be locked anymore
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Ok(release) if !release.is_locked()));
    }

    #[test]
    fn test_upgrade_with_lock_during_upgrade() {
        // We want to check that we manage to install a chart even if a lock is present while it not the first installation
        let HelmTestCtx {
            ref helm,
            ref mut charts,
        } = HelmTestCtx::new("test-upgrade-with-lock-upgrade");

        // check release does not exist yet
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name));

        // First install
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        // Spawn our task killer
        let barrier = Arc::new(Barrier::new(2));
        thread::spawn({
            let barrier = barrier.clone();
            let chart_name = charts[0].name.clone();
            move || {
                barrier.wait();
                thread::sleep(Duration::from_millis(5000));
                let mut cmd = QoveryCommand::new("pkill", &["-9", "-f", &format!("helm.*{chart_name}")], &[]);
                let _ = cmd.exec();
            }
        });

        charts[0].values = vec![ChartSetValue {
            key: "initialDelaySeconds".to_string(),
            value: "10".to_string(),
        }];
        barrier.wait();
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(ret.is_err());

        // Release should be locked
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Ok(release) if release.is_locked() && release.version == 2));

        // New installation should work even if a lock is present
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        // Release should not be locked anymore
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Ok(release) if !release.is_locked() && release.version == 4));
    }

    #[test]
    fn test_uninstall() {
        let HelmTestCtx { ref helm, ref charts } = HelmTestCtx::new("test-uninstall");

        // check release does not exist yet
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name));

        // deleting something that does not exist should not be an issue
        let ret = helm.uninstall(&charts[0], &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {});
        assert!(matches!(ret, Ok(())));

        // install it
        let ret = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        assert!(matches!(ret, Ok(())));

        // check now it exists
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(ret.is_ok());

        // Delete it
        let ret = helm.uninstall(&charts[0], &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {});
        assert!(matches!(ret, Ok(())));

        // check release does not exist anymore
        let ret = helm.check_release_exist(&charts[0], &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == charts[0].name));
    }

    #[test]
    fn test_getting_version() {
        let HelmTestCtx {
            ref helm,
            ref mut charts,
        } = HelmTestCtx::new("test-version-release");
        let _ = helm.upgrade(&charts[0], &[], &CommandKiller::never());
        let releases = helm.list_release(Some(&charts[0].get_namespace_string()), &[]).unwrap();
        assert_eq!(releases[0].clone().chart_version.unwrap(), Version::new(0, 1, 0))
    }

    #[test]
    fn test_fetching_chart() {
        let HelmTestCtx { ref helm, .. } = HelmTestCtx::new("test-download-chart");

        let target_dir = tempfile::tempdir().unwrap();
        let repository = Url::parse("https://kubernetes.github.io/ingress-nginx").unwrap();

        // Invalid target directory should fail
        let ret = helm.download_chart(
            &repository,
            &GenericCr {
                long_id: Uuid::new_v4(),
                url: repository.clone(),
                credentials: None,
            },
            "ingress-nginx",
            "4.4.9999",
            Path::new("/xxxxxxxxxx"),
            false,
            &[],
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Err(HelmError::CmdError(_, _, _))));

        // Non existing version should fail
        let ret = helm.download_chart(
            &repository,
            &GenericCr {
                long_id: Uuid::new_v4(),
                url: repository.clone(),
                credentials: None,
            },
            "ingress-nginx",
            "4.4.9999",
            target_dir.path(),
            false,
            &[],
            &CommandKiller::never(),
        );
        assert!(
            matches!(ret, Err(HelmError::CmdError(_, _, ref err)) if err.message_raw().unwrap().contains("version \"4.4.9999\" not found"))
        );

        // Happy path
        assert!(!target_dir.path().join("values.yaml").exists());
        let ret = helm.download_chart(
            &repository,
            &GenericCr {
                long_id: Uuid::new_v4(),
                url: repository.clone(),
                credentials: None,
            },
            "ingress-nginx",
            "4.4.2",
            target_dir.path(),
            false,
            &[],
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());

        // Check that the files are there
        assert!(target_dir.path().join("values.yaml").exists());
    }

    #[test]
    fn test_fetching_chart_generic_cr_public() {
        let HelmTestCtx { ref helm, .. } = HelmTestCtx::new("test-download-chart");

        let target_dir = tempfile::tempdir().unwrap();
        let repository = Url::parse("oci://registry-1.docker.io").unwrap();

        // Invalid target directory should fail
        let ret = helm.download_chart(
            &repository,
            &GenericCr {
                long_id: Uuid::new_v4(),
                url: repository.clone(),
                credentials: None,
            },
            "bitnamicharts/multus-cni",
            "1.1.7",
            Path::new("/xxxxxxxxxx"),
            false,
            &[],
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Err(HelmError::CmdError(_, _, _))));

        // Non existing version should fail
        let ret = helm.download_chart(
            &repository,
            &GenericCr {
                long_id: Uuid::new_v4(),
                url: repository.clone(),
                credentials: None,
            },
            "bitnamicharts/multus-cni",
            "invalid",
            target_dir.path(),
            false,
            &[],
            &CommandKiller::never(),
        );
        assert!(
            matches!(ret, Err(HelmError::CmdError(_, _, ref err)) if err.message_raw().unwrap().contains("invalid: Command terminated with a non success exit status code"))
        );

        // Happy path
        assert!(!target_dir.path().join("values.yaml").exists());
        let ret = helm.download_chart(
            &repository,
            &GenericCr {
                long_id: Uuid::new_v4(),
                url: repository.clone(),
                credentials: None,
            },
            "bitnamicharts/multus-cni",
            "1.1.7",
            target_dir.path(),
            false,
            &[],
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());

        // Check that the files are there
        assert!(target_dir.path().join("values.yaml").exists());
    }
}
