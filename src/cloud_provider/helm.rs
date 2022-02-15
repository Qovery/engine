use crate::cloud_provider::helm::HelmAction::Deploy;
use crate::cloud_provider::helm::HelmChartNamespaces::KubeSystem;
use crate::cloud_provider::qovery::{get_qovery_app_version, EngineLocation, QoveryAppName, QoveryShellAgent};
use crate::cmd::helm::{
    helm_destroy_chart_if_breaking_changes_version_detected, helm_exec_upgrade_with_chart_info,
    helm_upgrade_diff_with_chart_info, to_command_error, Helm,
};
use crate::cmd::kubectl::{
    kubectl_delete_crash_looping_pods, kubectl_exec_delete_crd, kubectl_exec_get_configmap, kubectl_exec_get_events,
    kubectl_exec_rollout_restart_deployment, kubectl_exec_with_output,
};
use crate::cmd::structs::HelmHistoryRow;
use crate::errors::CommandError;
use crate::utilities::calculate_hash;
use semver::Version;
use std::collections::HashMap;
use std::path::Path;
use std::{fs, thread};
use thread::spawn;
use tracing::{span, Level};
use uuid::Uuid;

#[derive(Clone)]
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

impl HelmChartNamespaces {
    pub fn to_string(&self) -> String {
        match self {
            HelmChartNamespaces::Custom => "custom",
            HelmChartNamespaces::KubeSystem => "kube-system",
            HelmChartNamespaces::Prometheus => "prometheus",
            HelmChartNamespaces::Logging => "logging",
            HelmChartNamespaces::CertManager => "cert-manager",
            HelmChartNamespaces::NginxIngress => "nginx-ingress",
            HelmChartNamespaces::Qovery => "qovery",
        }
        .to_string()
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
    pub last_breaking_version_requiring_restart: Option<Version>,
    pub timeout_in_seconds: i64,
    pub dry_run: bool,
    pub wait: bool,
    pub values: Vec<ChartSetValue>,
    pub values_files: Vec<String>,
    pub yaml_files_content: Vec<ChartValuesGenerated>,
    pub parse_stderr_for_error: bool,
    pub k8s_selector: Option<String>,
}

impl ChartInfo {
    pub fn new_from_custom_namespace(
        name: String,
        path: String,
        custom_namespace: String,
        timeout_in_seconds: i64,
        values_files: Vec<String>,
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
            timeout_in_seconds: 3600,
            atomic: true,
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
            last_breaking_version_requiring_restart: None,
            timeout_in_seconds: 300,
            dry_run: false,
            wait: true,
            values: Vec::new(),
            values_files: Vec::new(),
            yaml_files_content: vec![],
            parse_stderr_for_error: true,
            k8s_selector: None,
        }
    }
}

pub trait HelmChart: Send {
    fn check_prerequisites(&self) -> Result<Option<ChartPayload>, CommandError> {
        let chart = self.get_chart_info();
        for file in chart.values_files.iter() {
            if let Err(e) = fs::metadata(file) {
                let safe_message = format!(
                    "Can't access helm chart override file `{}` for chart `{}`",
                    file, chart.name,
                );
                return Err(CommandError::new(
                    format!("{}, error: {:?}", safe_message, e),
                    Some(safe_message),
                ));
            }
        }
        Ok(None)
    }

    fn get_selector(&self) -> Option<String> {
        let infos = self.get_chart_info();
        infos.k8s_selector.clone()
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
        let chart_infos = self.get_chart_info();

        // Cleaning any existing crash looping pod for this helm chart
        if let Some(selector) = self.get_selector() {
            kubectl_delete_crash_looping_pods(
                &kubernetes_config,
                Some(chart_infos.get_namespace_string().as_str()),
                Some(selector.as_str()),
                envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect(),
            )?;
        }

        Ok(payload)
    }

    fn run(&self, kubernetes_config: &Path, envs: &[(String, String)]) -> Result<Option<ChartPayload>, CommandError> {
        info!("prepare and deploy chart {}", &self.get_chart_info().name);
        let payload = self.check_prerequisites()?;
        let payload = self.pre_exec(&kubernetes_config, &envs, payload)?;
        let payload = match self.exec(&kubernetes_config, &envs, payload.clone()) {
            Ok(payload) => payload,
            Err(e) => {
                error!("Error while deploying chart: {}", e.message());
                self.on_deploy_failure(&kubernetes_config, &envs, payload)?;
                return Err(e);
            }
        };
        let payload = self.post_exec(&kubernetes_config, &envs, payload)?;
        Ok(payload)
    }

    fn exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        let environment_variables: Vec<(&str, &str)> = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
        let chart_info = self.get_chart_info();
        let helm = Helm::new(kubernetes_config).map_err(to_command_error)?;

        match chart_info.action {
            HelmAction::Deploy => {
                if let Err(e) = helm_destroy_chart_if_breaking_changes_version_detected(
                    kubernetes_config,
                    &environment_variables,
                    chart_info,
                ) {
                    warn!(
                        "error while trying to destroy chart if breaking change is detected: {:?}",
                        e.message()
                    );
                }

                helm_exec_upgrade_with_chart_info(kubernetes_config, &environment_variables, chart_info)?
            }
            HelmAction::Destroy => {
                let chart_info = self.get_chart_info();
                helm.uninstall(&chart_info).map_err(to_command_error)?;
            }
            HelmAction::Skip => {}
        }
        Ok(payload)
    }

    fn post_exec(
        &self,
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
        let environment_variables: Vec<(&str, &str)> = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
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

fn deploy_parallel_charts(
    kubernetes_config: &Path,
    envs: &[(String, String)],
    charts: Vec<Box<dyn HelmChart>>,
) -> Result<(), CommandError> {
    let mut handles = vec![];

    for chart in charts.into_iter() {
        let environment_variables = envs.to_owned();
        let path = kubernetes_config.to_path_buf();
        let current_span = tracing::Span::current();
        let handle = spawn(move || {
            // making sure to pass the current span to the new thread not to lose any tracing info
            span!(parent: &current_span, Level::INFO, "") // empty span name to reduce logs length
                .in_scope(|| chart.run(path.as_path(), &environment_variables))
        });
        handles.push(handle);
    }

    for handle in handles {
        match handle.join() {
            Ok(helm_run_ret) => {
                if let Err(e) = helm_run_ret {
                    return Err(e);
                }
            }
            Err(e) => {
                let safe_message = "Thread panicked during parallel charts deployments.";
                return Err(CommandError::new(
                    format!("{}, error: {:?}", safe_message.to_string(), e),
                    Some(safe_message.to_string()),
                ));
            }
        }
    }

    Ok(())
}

pub fn deploy_charts_levels(
    kubernetes_config: &Path,
    envs: &Vec<(String, String)>,
    charts: Vec<Vec<Box<dyn HelmChart>>>,
    dry_run: bool,
) -> Result<(), CommandError> {
    // first show diff
    for level in &charts {
        for chart in level {
            let chart_info = chart.get_chart_info();
            match chart_info.action {
                // don't do diff on destroy or skip
                HelmAction::Deploy => {
                    let _ = helm_upgrade_diff_with_chart_info(&kubernetes_config, envs, chart.get_chart_info());
                }
                _ => {}
            }
        }
    }

    // then apply
    if dry_run {
        return Ok(());
    }
    for level in charts.into_iter() {
        if let Err(e) = deploy_parallel_charts(&kubernetes_config, &envs, level) {
            return Err(e);
        }
    }

    Ok(())
}

//
// Common charts
//

#[derive(Default)]
pub struct CommonChart {
    pub chart_info: ChartInfo,
}

/// using ChartPayload to pass random kind of data between each deployment steps against a chart deployment
#[derive(Clone)]
pub struct ChartPayload {
    data: HashMap<String, String>,
}

impl CommonChart {}

impl HelmChart for CommonChart {
    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }
}

// CoreDNS config

#[derive(Default)]
pub struct CoreDNSConfigChart {
    pub chart_info: ChartInfo,
}

impl HelmChart for CoreDNSConfigChart {
    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    fn pre_exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        _payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        let kind = "configmap";
        let mut environment_variables: Vec<(&str, &str)> = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
        environment_variables.push(("KUBECONFIG", kubernetes_config.to_str().unwrap()));

        let chart_infos = self.get_chart_info();

        // Cleaning any existing crash looping pod for this helm chart
        if let Some(selector) = self.get_selector() {
            kubectl_delete_crash_looping_pods(
                &kubernetes_config,
                Some(chart_infos.get_namespace_string().as_str()),
                Some(selector.as_str()),
                environment_variables.clone(),
            )?;
        }

        // calculate current configmap checksum
        let current_configmap_hash = match kubectl_exec_get_configmap(
            &kubernetes_config,
            &self.chart_info.get_namespace_string(),
            &self.chart_info.name,
            environment_variables.clone(),
        ) {
            Ok(cm) => {
                if cm.data.corefile.is_none() {
                    return Err(CommandError::new_from_safe_message(
                        "Corefile data structure is not found in CoreDNS configmap".to_string(),
                    ));
                };
                calculate_hash(&cm.data.corefile.unwrap())
            }
            Err(e) => return Err(e),
        };
        let mut configmap_hash = HashMap::new();
        configmap_hash.insert("checksum".to_string(), current_configmap_hash.to_string());
        let payload = ChartPayload { data: configmap_hash };

        // set labels and annotations to give helm ownership
        info!("setting annotations and labels on {}/{}", &kind, &self.chart_info.name);
        let steps = || -> Result<(), CommandError> {
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "annotate",
                    "--overwrite",
                    &kind,
                    &self.chart_info.name,
                    format!("meta.helm.sh/release-name={}", self.chart_info.name).as_str(),
                ],
                environment_variables.clone(),
                |_| {},
                |_| {},
            )?;
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "annotate",
                    "--overwrite",
                    &kind,
                    &self.chart_info.name,
                    "meta.helm.sh/release-namespace=kube-system",
                ],
                environment_variables.clone(),
                |_| {},
                |_| {},
            )?;
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "label",
                    "--overwrite",
                    &kind,
                    &self.chart_info.name,
                    "app.kubernetes.io/managed-by=Helm",
                ],
                environment_variables.clone(),
                |_| {},
                |_| {},
            )?;
            Ok(())
        };
        if let Err(e) = steps() {
            return Err(e);
        };

        Ok(Some(payload))
    }

    fn run(&self, kubernetes_config: &Path, envs: &[(String, String)]) -> Result<Option<ChartPayload>, CommandError> {
        info!("prepare and deploy chart {}", &self.get_chart_info().name);
        self.check_prerequisites()?;
        let payload = match self.pre_exec(&kubernetes_config, &envs, None) {
            Ok(p) => match p {
                None => {
                    return Err(CommandError::new_from_safe_message(
                        "CoreDNS configmap checksum couldn't be get, can't deploy CoreDNS chart".to_string(),
                    ))
                }
                Some(p) => p,
            },
            Err(e) => return Err(e),
        };
        if let Err(e) = self.exec(&kubernetes_config, &envs, None) {
            error!("Error while deploying chart: {:?}", e.message());
            self.on_deploy_failure(&kubernetes_config, &envs, None)?;
            return Err(e);
        };
        self.post_exec(&kubernetes_config, &envs, Some(payload))?;
        Ok(None)
    }

    fn post_exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        let mut environment_variables = Vec::new();
        for env in envs {
            environment_variables.push((env.0.as_str(), env.1.as_str()));
        }

        // detect configmap data change
        let previous_configmap_checksum = match &payload {
            None => {
                return Err(CommandError::new_from_safe_message(
                    "Missing payload, can't check coredns update".to_string(),
                ))
            }
            Some(x) => match x.data.get("checksum") {
                None => {
                    return Err(CommandError::new_from_safe_message(
                        "Missing configmap checksum, can't check coredns diff".to_string(),
                    ))
                }
                Some(c) => c.clone(),
            },
        };
        let current_configmap_checksum = match kubectl_exec_get_configmap(
            &kubernetes_config,
            &self.chart_info.get_namespace_string(),
            &self.chart_info.name,
            environment_variables.clone(),
        ) {
            Ok(cm) => {
                if cm.data.corefile.is_none() {
                    return Err(CommandError::new_from_safe_message(
                        "Corefile data structure is not found in CoreDNS configmap".to_string(),
                    ));
                };
                calculate_hash(&cm.data.corefile.unwrap()).to_string()
            }
            Err(e) => return Err(e),
        };
        if previous_configmap_checksum == current_configmap_checksum {
            info!("no coredns config change detected, nothing to restart");
            return Ok(None);
        }

        // avoid rebooting coredns on every run
        info!("coredns config change detected, proceed to config reload");
        kubectl_exec_rollout_restart_deployment(
            kubernetes_config,
            &self.chart_info.name,
            self.namespace().as_str(),
            &environment_variables,
        )?;
        Ok(None)
    }
}

// Prometheus Operator

#[derive(Default)]
pub struct PrometheusOperatorConfigChart {
    pub chart_info: ChartInfo,
}

impl HelmChart for PrometheusOperatorConfigChart {
    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    fn exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        let environment_variables: Vec<(&str, &str)> = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
        let chart_info = self.get_chart_info();
        let helm = Helm::new(kubernetes_config).map_err(to_command_error)?;

        match chart_info.action {
            HelmAction::Deploy => {
                if let Err(e) = helm_destroy_chart_if_breaking_changes_version_detected(
                    kubernetes_config,
                    &environment_variables,
                    chart_info,
                ) {
                    warn!(
                        "error while trying to destroy chart if breaking change is detected: {}",
                        e.message()
                    );
                }

                helm_exec_upgrade_with_chart_info(kubernetes_config, &environment_variables, chart_info)?
            }
            HelmAction::Destroy => {
                let chart_info = self.get_chart_info();
                helm.uninstall(&chart_info).map_err(to_command_error)?;

                let prometheus_crds = [
                    "prometheuses.monitoring.coreos.com",
                    "prometheusrules.monitoring.coreos.com",
                    "servicemonitors.monitoring.coreos.com",
                    "podmonitors.monitoring.coreos.com",
                    "alertmanagers.monitoring.coreos.com",
                    "thanosrulers.monitoring.coreos.com",
                ];
                for crd in &prometheus_crds {
                    let _ = kubectl_exec_delete_crd(kubernetes_config, crd, environment_variables.clone());
                }
            }
            HelmAction::Skip => {}
        }
        Ok(payload)
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
        EngineLocation::ClientSide => HelmAction::Deploy,
        EngineLocation::QoverySide => HelmAction::Destroy,
    }
}

pub struct ShellAgentContext<'a> {
    pub api_url: &'a str,
    pub api_token: &'a str,
    pub organization_long_id: &'a Uuid,
    pub cluster_id: &'a str,
    pub cluster_long_id: &'a Uuid,
    pub cluster_token: &'a str,
    pub grpc_url: &'a str,
}

pub fn get_chart_for_shell_agent(
    context: ShellAgentContext,
    chart_path: impl Fn(&str) -> String,
) -> Result<CommonChart, CommandError> {
    let shell_agent_version: QoveryShellAgent = get_qovery_app_version(
        QoveryAppName::ShellAgent,
        context.api_token,
        context.api_url,
        context.cluster_id,
    )?;
    let shell_agent = CommonChart {
        chart_info: ChartInfo {
            name: "shell-agent".to_string(),
            path: chart_path("common/charts/qovery-shell-agent"),
            namespace: HelmChartNamespaces::Qovery,
            values: vec![
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: shell_agent_version.version,
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
                    value: "DEBUG".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.GRPC_SERVER".to_string(),
                    value: context.grpc_url.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLUSTER_TOKEN".to_string(),
                    value: context.cluster_token.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLUSTER_ID".to_string(),
                    value: context.cluster_long_id.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.ORGANIZATION_ID".to_string(),
                    value: context.organization_long_id.to_string(),
                },
                // resources limits
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
            ],
            ..Default::default()
        },
    };

    Ok(shell_agent)
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
