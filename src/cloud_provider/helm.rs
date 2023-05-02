use crate::cloud_provider::helm::HelmAction::Deploy;
use crate::cloud_provider::helm::HelmChartNamespaces::KubeSystem;
use crate::cloud_provider::qovery::EngineLocation;
use crate::cmd::helm::{to_command_error, Helm};
use crate::cmd::helm_utils::{
    apply_chart_backup, delete_unused_chart_backup, prepare_chart_backup_on_upgrade, update_crds_on_upgrade,
    BackupStatus, CRDSUpdate,
};
use crate::cmd::kubectl::{kubectl_delete_crash_looping_pods, kubectl_exec_delete_crd, kubectl_exec_get_events};
use crate::cmd::structs::HelmHistoryRow;
use crate::errors::{CommandError, ErrorMessageVerbosity};

use semver::Version;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use crate::cmd::command::CommandKiller;
use crate::deployment_action::deploy_helm::default_helm_timeout;
use std::{fs, thread};
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq)]
pub enum HelmAction {
    Deploy,
    Destroy,
    Skip,
}

#[derive(Copy, Clone)]
pub enum HelmChartNamespaces {
    KubeSystem,
    Prometheus,
    Logging,
    CertManager,
    NginxIngress,
    Qovery,
    Custom,
}

impl Display for HelmChartNamespaces {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            HelmChartNamespaces::Custom => "custom",
            KubeSystem => "kube-system",
            HelmChartNamespaces::Prometheus => "prometheus",
            HelmChartNamespaces::Logging => "logging",
            HelmChartNamespaces::CertManager => "cert-manager",
            HelmChartNamespaces::NginxIngress => "nginx-ingress",
            HelmChartNamespaces::Qovery => "qovery",
        };

        f.write_str(str)
    }
}

#[derive(Clone)]
pub struct ChartSetValue {
    pub key: String,
    pub value: String,
}

#[derive(Clone)]
pub struct ChartValuesGenerated {
    pub filename: String,
    pub yaml_content: String,
}

#[derive(Clone)]
pub struct ChartInfo {
    pub name: String,
    pub path: String,
    pub namespace: HelmChartNamespaces,
    pub custom_namespace: Option<String>,
    pub action: HelmAction,
    pub atomic: bool,
    pub force_upgrade: bool,
    pub recreate_pods: bool,
    pub reinstall_chart_if_installed_version_is_below_than: Option<Version>,
    pub timeout_in_seconds: i64,
    pub dry_run: bool,
    pub wait: bool,
    /// Values used to override values set inside values files.
    pub values: Vec<ChartSetValue>,
    pub values_string: Vec<ChartSetValue>,
    pub values_files: Vec<String>,
    pub yaml_files_content: Vec<ChartValuesGenerated>,
    pub parse_stderr_for_error: bool,
    pub k8s_selector: Option<String>,
    pub backup_resources: Option<Vec<String>>,
    pub crds_update: Option<CRDSUpdate>,
}

impl ChartInfo {
    pub fn new_from_custom_namespace(
        name: String,
        path: String,
        custom_namespace: String,
        timeout_in_seconds: i64,
        values_files: Vec<String>,
        values: Vec<ChartSetValue>,
        values_string: Vec<ChartSetValue>,
        parse_stderr_for_error: bool,
        k8s_selector: Option<String>,
    ) -> Self {
        ChartInfo {
            name,
            path,
            namespace: HelmChartNamespaces::Custom,
            custom_namespace: Some(custom_namespace),
            timeout_in_seconds,
            values_files,
            values,
            values_string,
            parse_stderr_for_error,
            k8s_selector,
            ..Default::default()
        }
    }

    pub fn new_from_release_name(name: &str, custom_namespace: &str) -> ChartInfo {
        ChartInfo {
            name: name.to_string(),
            namespace: HelmChartNamespaces::Custom,
            custom_namespace: Some(custom_namespace.to_string()),
            ..Default::default()
        }
    }

    pub fn get_namespace_string(&self) -> String {
        match self.namespace {
            HelmChartNamespaces::Custom => self
                .custom_namespace
                .clone()
                .unwrap_or_else(|| self.namespace.to_string()),
            _ => self.namespace.to_string(),
        }
    }
}

impl Default for ChartInfo {
    fn default() -> ChartInfo {
        ChartInfo {
            name: "undefined".to_string(),
            path: "undefined".to_string(),
            namespace: KubeSystem,
            custom_namespace: None,
            action: Deploy,
            atomic: true,
            force_upgrade: false,
            recreate_pods: false,
            reinstall_chart_if_installed_version_is_below_than: None,
            timeout_in_seconds: default_helm_timeout().as_secs() as i64,
            dry_run: false,
            wait: true,
            values: vec![],
            values_string: vec![], // values to force string usage
            values_files: vec![],
            yaml_files_content: vec![],
            parse_stderr_for_error: true,
            k8s_selector: None,
            backup_resources: None,
            crds_update: None,
        }
    }
}

pub trait HelmChart: Send {
    fn clone_dyn(&self) -> Box<dyn HelmChart>;

    fn check_prerequisites(&self) -> Result<Option<ChartPayload>, CommandError> {
        let chart = self.get_chart_info();
        for file in chart.values_files.iter() {
            if let Err(e) = fs::metadata(file) {
                return Err(CommandError::new(
                    format!("Can't access helm chart override file `{}` for chart `{}`", file, chart.name,),
                    Some(e.to_string()),
                    None,
                ));
            }
        }
        Ok(None)
    }

    fn get_selector(&self) -> &Option<String> {
        &self.get_chart_info().k8s_selector
    }

    fn get_chart_info(&self) -> &ChartInfo;

    fn namespace(&self) -> String {
        self.get_chart_info().get_namespace_string()
    }

    fn pre_exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        // Cleaning any existing crash looping pod for this helm chart
        if let Some(selector) = self.get_selector() {
            kubectl_delete_crash_looping_pods(
                kubernetes_config,
                Some(self.get_chart_info().get_namespace_string().as_str()),
                Some(selector.as_str()),
                envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect(),
            )?;
        }

        Ok(payload)
    }

    fn run(
        &self,
        kube_client: &kube::Client,
        kubernetes_config: &Path,
        envs: &[(String, String)],
    ) -> Result<Option<ChartPayload>, CommandError> {
        info!("prepare and deploy chart {}", &self.get_chart_info().name);
        let payload = self.check_prerequisites()?;
        let payload = self.pre_exec(kubernetes_config, envs, payload)?;
        let payload = match self.exec(kubernetes_config, envs, payload.clone()) {
            Ok(payload) => payload,
            Err(e) => {
                error!("Error while deploying chart: {}", e.message(ErrorMessageVerbosity::FullDetails));
                self.on_deploy_failure(kubernetes_config, envs, payload)?;
                return Err(e);
            }
        };
        let payload = self.post_exec(kube_client, kubernetes_config, envs, payload)?;
        Ok(payload)
    }

    fn exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        let environment_variables: Vec<(&str, &str)> = envs.iter().map(|(l, r)| (l.as_str(), r.as_str())).collect();
        let chart_info = self.get_chart_info();
        let helm = Helm::new(kubernetes_config, &environment_variables).map_err(to_command_error)?;

        match chart_info.action {
            Deploy => {
                if let Err(e) = helm.uninstall_chart_if_breaking_version(chart_info, &[]) {
                    warn!(
                        "error while trying to destroy chart if breaking change is detected: {:?}",
                        e.to_string()
                    );
                }

                let installed_chart_version = match helm.get_chart_version(
                    &chart_info.name,
                    Some(chart_info.get_namespace_string().as_str()),
                    environment_variables.as_slice(),
                ) {
                    Ok(versions) => match versions {
                        None => None,
                        Some(versions) => versions.chart_version,
                    },
                    Err(e) => {
                        warn!("error while trying to get installed version: {:?}", e);
                        None
                    }
                };

                let upgrade_status = match prepare_chart_backup_on_upgrade(
                    kubernetes_config,
                    chart_info.clone(),
                    environment_variables.as_slice(),
                    installed_chart_version,
                ) {
                    Ok(status) => status,
                    Err(e) => {
                        warn!("error while trying to prepare backup: {:?}", e);
                        BackupStatus {
                            is_backupable: false,
                            backup_path: PathBuf::new(),
                        }
                    }
                };

                // Verify that we don't need to upgrade the CRDS
                update_crds_on_upgrade(kubernetes_config, chart_info.clone(), environment_variables.as_slice(), &helm)
                    .map_err(to_command_error)?;

                match helm
                    .upgrade(chart_info, &[], &CommandKiller::never())
                    .map_err(to_command_error)
                {
                    Ok(_) => {
                        if upgrade_status.is_backupable {
                            if let Err(e) = apply_chart_backup(
                                kubernetes_config,
                                upgrade_status.backup_path.as_path(),
                                environment_variables.as_slice(),
                                chart_info,
                            ) {
                                warn!("error while trying to apply backup: {:?}", e);
                            };
                        }
                    }
                    Err(e) => {
                        if upgrade_status.is_backupable {
                            if let Err(e) = delete_unused_chart_backup(
                                kubernetes_config,
                                environment_variables.as_slice(),
                                chart_info,
                            ) {
                                warn!("error while trying to delete backup: {:?}", e);
                            }
                        }

                        return Err(e);
                    }
                }
            }
            HelmAction::Destroy => {
                let chart_info = self.get_chart_info();
                if let Some(crds_update) = &chart_info.crds_update {
                    // FIXME: This can't work as crd as .yaml in the string
                    for crd in &crds_update.resources {
                        if let Err(e) =
                            kubectl_exec_delete_crd(kubernetes_config, crd.as_str(), environment_variables.clone())
                        {
                            warn!("error while trying to delete crd {}: {:?}", crd, e);
                        }
                    }
                }
                helm.uninstall(chart_info, &[]).map_err(to_command_error)?;
            }
            HelmAction::Skip => {}
        }
        Ok(payload)
    }

    fn post_exec(
        &self,
        _kube_client: &kube::Client,
        _kubernetes_config: &Path,
        _envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        Ok(payload)
    }

    fn on_deploy_failure(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        // print events for future investigation
        let environment_variables: Vec<(&str, &str)> = envs.iter().map(|(l, r)| (l.as_str(), r.as_str())).collect();
        match kubectl_exec_get_events(
            kubernetes_config,
            Some(self.get_chart_info().get_namespace_string().as_str()),
            environment_variables,
        ) {
            Ok(ok_line) => info!("{}", ok_line),
            Err(err) => {
                error!("{:?}", err);
                return Err(err);
            }
        };
        Ok(payload)
    }
}

impl Clone for Box<dyn HelmChart> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

fn deploy_parallel_charts(
    kube_client: &kube::Client,
    kubernetes_config: &Path,
    envs: &[(String, String)],
    charts: Vec<Box<dyn HelmChart>>,
) -> Result<(), CommandError> {
    thread::scope(|s| {
        let mut handles = vec![];

        for chart in charts.into_iter() {
            let environment_variables = envs.to_owned();
            let path = kubernetes_config.to_path_buf();
            let current_span = tracing::Span::current();
            let handle = s.spawn(move || {
                // making sure to pass the current span to the new thread not to lose any tracing info
                let _ = current_span.enter();
                chart.run(kube_client, path.as_path(), &environment_variables)
            });

            handles.push(handle);
        }

        let mut errors: Vec<Result<(), CommandError>> = vec![];
        for handle in handles {
            match handle.join() {
                Ok(helm_run_ret) => {
                    if let Err(e) = helm_run_ret {
                        errors.push(Err(e));
                    }
                }
                Err(e) => {
                    let err = match e.downcast_ref::<&'static str>() {
                        None => match e.downcast_ref::<String>() {
                            None => "Unable to get error.",
                            Some(s) => s.as_str(),
                        },
                        Some(s) => *s,
                    };
                    let error = Err(CommandError::new(
                        "Thread panicked during parallel charts deployments.".to_string(),
                        Some(err.to_string()),
                        None,
                    ));
                    errors.push(error);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            error!("Deployments of charts failed with: {:?}", errors);
            errors.remove(0)
        }
    })
}

pub fn deploy_charts_levels(
    kube_client: &kube::Client,
    kubernetes_config: &Path,
    envs: &[(String, String)],
    charts: Vec<Vec<Box<dyn HelmChart>>>,
    dry_run: bool,
) -> Result<(), CommandError> {
    // first show diff
    let envs_ref: Vec<(&str, &str)> = envs.iter().map(|(x, y)| (x.as_str(), y.as_str())).collect();
    let helm = Helm::new(kubernetes_config, &envs_ref).map_err(to_command_error)?;

    for level in charts {
        // Show diff for all chart in this state
        for chart in &level {
            let chart_info = chart.get_chart_info();
            // don't do diff on destroy or skip
            if chart_info.action == Deploy {
                let _ = helm.upgrade_diff(chart_info, &[]);
            }
        }

        // Skip actual deployment if dry run
        if dry_run {
            continue;
        }

        deploy_parallel_charts(kube_client, kubernetes_config, envs, level)?
    }

    Ok(())
}

//
// Common charts
//

pub trait ChartInstallationChecker: Send {
    fn verify_installation(&self, kube_client: &kube::Client) -> Result<(), CommandError>;
    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker>;
}

impl Clone for Box<dyn ChartInstallationChecker> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

#[derive(Default, Clone)]
pub struct CommonChart {
    pub chart_info: ChartInfo,
    pub chart_installation_checker: Option<Box<dyn ChartInstallationChecker>>,
}

/// using ChartPayload to pass random kind of data between each deployment steps against a chart deployment
#[derive(Clone)]
pub struct ChartPayload {
    data: HashMap<String, String>,
}

impl ChartPayload {
    pub fn new(data: HashMap<String, String>) -> ChartPayload {
        ChartPayload { data }
    }

    pub fn data(&self) -> &HashMap<String, String> {
        &self.data
    }
}

impl HelmChart for CommonChart {
    fn clone_dyn(&self) -> Box<dyn HelmChart> {
        Box::new(self.clone())
    }

    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    fn post_exec(
        &self,
        kube_client: &kube::Client,
        _kubernetes_config: &Path,
        _envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        match &self.chart_installation_checker {
            Some(checker) => match checker.verify_installation(kube_client) {
                Ok(_) => Ok(payload),
                Err(e) => Err(e),
            },
            // If no checker set, then consider it's ok
            None => Ok(payload),
        }
    }
}

pub fn get_latest_successful_deployment(helm_history_list: &[HelmHistoryRow]) -> Result<HelmHistoryRow, CommandError> {
    let mut helm_history_reversed = helm_history_list.to_owned();
    helm_history_reversed.reverse();

    for revision in helm_history_reversed.clone() {
        if revision.status == "deployed" {
            return Ok(revision);
        }
    }

    Err(CommandError::new_from_safe_message(format!(
        "No succeed revision found for chart `{}`",
        helm_history_reversed[0].chart
    )))
}

pub fn get_engine_helm_action_from_location(location: &EngineLocation) -> HelmAction {
    match location {
        EngineLocation::ClientSide => Deploy,
        EngineLocation::QoverySide => HelmAction::Destroy,
    }
}

// Shell Agent

pub struct ShellAgentContext<'a> {
    pub version: String,
    pub api_url: &'a str,
    pub organization_long_id: &'a Uuid,
    pub cluster_id: &'a str,
    pub cluster_long_id: &'a Uuid,
    pub cluster_jwt_token: &'a str,
    pub grpc_url: &'a str,
}

pub fn get_chart_for_shell_agent(
    context: ShellAgentContext,
    chart_path: impl Fn(&str) -> String,
    custom_resources: Option<Vec<ChartSetValue>>,
) -> Result<CommonChart, CommandError> {
    let mut shell_agent = CommonChart {
        chart_info: ChartInfo {
            name: "shell-agent".to_string(),
            path: chart_path("common/charts/qovery/qovery-shell-agent"),
            namespace: HelmChartNamespaces::Qovery,
            values: vec![
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: context.version,
                },
                ChartSetValue {
                    key: "replicaCount".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.RUST_BACKTRACE".to_string(),
                    value: "full".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.RUST_LOG".to_string(),
                    value: "h2::codec::framed_write=INFO\\,INFO".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.GRPC_SERVER".to_string(),
                    value: context.grpc_url.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLUSTER_JWT_TOKEN".to_string(),
                    value: context.cluster_jwt_token.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLUSTER_ID".to_string(),
                    value: context.cluster_long_id.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.ORGANIZATION_ID".to_string(),
                    value: context.organization_long_id.to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    // resources limits
    match custom_resources {
        None => {
            let mut default_resources = vec![
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "500Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "100Mi".to_string(),
                },
            ];
            shell_agent.chart_info.values.append(&mut default_resources)
        }
        Some(custom_resources) => {
            let mut custom_resources_tmp = custom_resources;
            shell_agent.chart_info.values.append(&mut custom_resources_tmp)
        }
    }

    Ok(shell_agent)
}

// Cluster Agent

pub struct ClusterAgentContext<'a> {
    pub version: String,
    pub api_url: &'a str,
    pub organization_long_id: &'a Uuid,
    pub cluster_id: &'a str,
    pub cluster_long_id: &'a Uuid,
    pub cluster_jwt_token: &'a str,
    pub grpc_url: &'a str,
    pub loki_url: Option<&'a str>,
}

// This one is the new agent in rust
pub fn get_chart_for_cluster_agent(
    context: ClusterAgentContext,
    chart_path: impl Fn(&str) -> String,
    custom_resources: Option<Vec<ChartSetValue>>,
) -> Result<CommonChart, CommandError> {
    let mut cluster_agent = CommonChart {
        chart_info: ChartInfo {
            name: "cluster-agent".to_string(),
            path: chart_path("common/charts/qovery/qovery-cluster-agent"),
            namespace: HelmChartNamespaces::Qovery,
            values: vec![
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: context.version,
                },
                ChartSetValue {
                    key: "replicaCount".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.RUST_BACKTRACE".to_string(),
                    value: "full".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.RUST_LOG".to_string(),
                    value: "h2::codec::framed_write=INFO\\,INFO".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.GRPC_SERVER".to_string(),
                    value: context.grpc_url.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLUSTER_JWT_TOKEN".to_string(),
                    value: context.cluster_jwt_token.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLUSTER_ID".to_string(),
                    value: context.cluster_long_id.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.ORGANIZATION_ID".to_string(),
                    value: context.organization_long_id.to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    // If log history is enabled, add the loki url to the values
    if let Some(url) = context.loki_url {
        cluster_agent.chart_info.values.push(ChartSetValue {
            key: "environmentVariables.LOKI_URL".to_string(),
            value: url.to_string(),
        });
    }

    // resources limits
    match custom_resources {
        None => {
            let mut default_resources = vec![
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "100Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "500Mi".to_string(),
                },
            ];
            cluster_agent.chart_info.values.append(&mut default_resources)
        }
        Some(custom_resources) => {
            let mut custom_resources_tmp = custom_resources;
            cluster_agent.chart_info.values.append(&mut custom_resources_tmp)
        }
    }

    Ok(cluster_agent)
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::get_latest_successful_deployment;
    use crate::cmd::structs::HelmHistoryRow;

    #[test]
    fn test_last_succeeded_deployment() {
        let payload = r#"
        [
            {
                "revision": 1,
                "updated": "2021-06-17T08:37:37.687890192+02:00",
                "status": "superseded",
                "chart": "coredns-config-0.1.0",
                "app_version": "0.1",
                "description": "Install complete"
            },
            {
                "revision": 2,
                "updated": "2021-06-17T12:34:08.958006444+02:00",
                "status": "deployed",
                "chart": "coredns-config-0.1.0",
                "app_version": "0.1",
                "description": "Upgrade complete"
            },
            {
                "revision": 3,
                "updated": "2021-06-17T12:36:08.958006444+02:00",
                "status": "failed",
                "chart": "coredns-config-0.1.0",
                "app_version": "0.1",
                "description": "Failed complete"
            }
        ]
        "#;

        let results = serde_json::from_str::<Vec<HelmHistoryRow>>(payload).unwrap();
        let final_succeed = get_latest_successful_deployment(&results).unwrap();
        assert_eq!(results[1].updated, final_succeed.updated);
    }
}
