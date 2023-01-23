use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartPayload, ChartSetValue, HelmAction, HelmChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath};
use crate::cmd::kubectl::{
    kubectl_delete_crash_looping_pods, kubectl_exec_get_configmap, kubectl_exec_rollout_restart_deployment,
    kubectl_exec_with_output,
};
use crate::errors::{CommandError, ErrorMessageVerbosity};
use crate::runtime::block_on;
use crate::utilities::calculate_hash;
use k8s_openapi::api::core::v1::Pod;
use kube::core::params::ListParams;
use kube::{Api, Client, ResourceExt};
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;
use std::collections::HashMap;
use std::path::Path;

// TODO(benjaminch): refactor this chart to have only one in common (issue with labels)
pub struct CoreDNSConfigChart {
    pub chart_info: ChartInfo,
    _chart_path: HelmChartPath,
    _chart_values_path: HelmChartValuesFilePath,
    chart_installation_checker: CoreDNSConfigChartChecker,
}

impl CoreDNSConfigChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        declare_hosts: bool,
        managed_dns_helm_format: String,
        managed_dns_resolvers_terraform_format: String,
    ) -> CoreDNSConfigChart {
        let chart_path = HelmChartPath::new(
            chart_prefix_path,
            HelmChartDirectoryLocation::CloudProviderFolder,
            format!("{}-config", CoreDNSConfigChart::chart_name()),
        );
        let chart_values_path = HelmChartValuesFilePath::new(
            chart_prefix_path,
            HelmChartDirectoryLocation::CloudProviderFolder,
            format!("{}-config", CoreDNSConfigChart::chart_name()),
        );

        CoreDNSConfigChart {
            _chart_path: chart_path.clone(),
            _chart_values_path: chart_values_path.clone(),
            chart_info: ChartInfo {
                name: CoreDNSConfigChart::chart_name(),
                path: chart_path.to_string(),
                namespace: HelmChartNamespaces::KubeSystem,
                custom_namespace: None,
                action: HelmAction::Deploy,
                atomic: false,
                force_upgrade: false,
                recreate_pods: false,
                last_breaking_version_requiring_restart: None,
                timeout_in_seconds: 0,
                dry_run: false,
                wait: false,
                values_files: vec![chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "declare_node_hosts".to_string(),
                        value: declare_hosts.to_string(),
                    },
                    ChartSetValue {
                        key: "managed_dns".to_string(),
                        value: managed_dns_helm_format,
                    },
                    ChartSetValue {
                        key: "managed_dns_resolvers".to_string(),
                        value: managed_dns_resolvers_terraform_format,
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: CoreDNSConfigChartChecker::new(),
        }
    }

    fn chart_name() -> String {
        "coredns".to_string()
    }
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
                kubernetes_config,
                Some(chart_infos.get_namespace_string().as_str()),
                Some(selector.as_str()),
                environment_variables.clone(),
            )?;
        }

        // calculate current configmap checksum
        let current_configmap_hash = match kubectl_exec_get_configmap(
            kubernetes_config,
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
        let payload = ChartPayload::new(configmap_hash);

        // set labels and annotations to give helm ownership
        info!("setting annotations and labels on {}/{}", &kind, &self.chart_info.name);
        let steps = || -> Result<(), CommandError> {
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "annotate",
                    "--overwrite",
                    kind,
                    &self.chart_info.name,
                    format!("meta.helm.sh/release-name={}", self.chart_info.name).as_str(),
                ],
                environment_variables.clone(),
                &mut |_| {},
                &mut |_| {},
            )?;
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "annotate",
                    "--overwrite",
                    kind,
                    &self.chart_info.name,
                    "meta.helm.sh/release-namespace=kube-system",
                ],
                environment_variables.clone(),
                &mut |_| {},
                &mut |_| {},
            )?;
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "label",
                    "--overwrite",
                    kind,
                    &self.chart_info.name,
                    "app.kubernetes.io/managed-by=Helm",
                ],
                environment_variables.clone(),
                &mut |_| {},
                &mut |_| {},
            )?;
            Ok(())
        };
        steps()?;
        Ok(Some(payload))
    }

    fn run(
        &self,
        kube_client: &kube::Client,
        kubernetes_config: &Path,
        envs: &[(String, String)],
    ) -> Result<Option<ChartPayload>, CommandError> {
        info!("prepare and deploy chart {}", &self.get_chart_info().name);
        self.check_prerequisites()?;
        let payload = match self.pre_exec(kubernetes_config, envs, None) {
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
        if let Err(e) = self.exec(kubernetes_config, envs, None) {
            error!(
                "Error while deploying chart: {:?}",
                e.message(ErrorMessageVerbosity::FullDetails)
            );
            self.on_deploy_failure(kubernetes_config, envs, None)?;
            return Err(e);
        };
        self.post_exec(kube_client, kubernetes_config, envs, Some(payload))?;
        Ok(None)
    }

    fn post_exec(
        &self,
        kube_client: &kube::Client,
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
            Some(x) => match x.data().get("checksum") {
                None => {
                    return Err(CommandError::new_from_safe_message(
                        "Missing configmap checksum, can't check coredns diff".to_string(),
                    ))
                }
                Some(c) => c.clone(),
            },
        };
        let current_configmap_checksum = match kubectl_exec_get_configmap(
            kubernetes_config,
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

        self.chart_installation_checker.verify_installation(kube_client)?;

        Ok(None)
    }
}

struct CoreDNSConfigChartChecker {}

impl CoreDNSConfigChartChecker {
    pub fn new() -> CoreDNSConfigChartChecker {
        CoreDNSConfigChartChecker {}
    }
}

impl ChartInstallationChecker for CoreDNSConfigChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        // This is a verify basic check: make sure CoreDNS pod is running
        let pods: Api<Pod> = Api::all(kube_client.clone());

        let result = retry::retry(Fixed::from_millis(5000).take(5), || {
            match block_on(pods.list(&ListParams::default().labels("k8s-app=kube-dns"))) {
                Ok(coredns_pods_result) => {
                    let mut err = Ok(());

                    // if no pods are there, then there is an issue
                    if coredns_pods_result.items.is_empty() {
                        err = Err(CommandError::new("No CoreDNS pods running".to_string(), None, None));
                    }

                    // check all CoreDNS pods are running properly
                    for coredns_pod in coredns_pods_result.items {
                        let mut pod_status_string = "UNKNOWN".to_string();
                        if let Some(pod_status) = &coredns_pod.status {
                            if let Some(pod_container_phase) = &pod_status.phase {
                                pod_status_string = pod_container_phase.trim().to_uppercase();
                                if pod_status_string == "RUNNING" {
                                    continue;
                                }
                            }
                        }

                        err = Err(CommandError::new(
                            format!(
                                "CoreDNS pod `{}` is not running but `{}`",
                                &coredns_pod.name(),
                                pod_status_string
                            ),
                            None,
                            None,
                        ));
                    }

                    match err {
                        Ok(_) => OperationResult::Ok(()),
                        Err(e) => OperationResult::Retry(e),
                    }
                }
                Err(e) => OperationResult::Retry(CommandError::new(
                    "Error trying to get CoreDNS pods".to_string(),
                    Some(e.to_string()),
                    None,
                )),
            }
        });

        match result {
            Ok(_) => Ok(()),
            Err(Operation { error, .. }) => Err(error),
            Err(retry::Error::Internal(e)) => {
                Err(CommandError::new("Error trying to get CoreDNS pods".to_string(), Some(e), None))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::CommonChart;
    use crate::cloud_provider::helm_charts::coredns_config_chart::CoreDNSConfigChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType,
    };
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn coredns_config_chart_directory_exists_test() {
        // setup:
        let chart = CoreDNSConfigChart::new(None, false, "whatever".to_string(), "whatever".to_string());

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}-config/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart._chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
            ),
            CoreDNSConfigChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }

    /// Makes sure chart values file exists.
    #[test]
    fn coredns_config_chart_values_file_exists_test() {
        // setup:
        let chart = CoreDNSConfigChart::new(None, false, "whatever".to_string(), "whatever".to_string());

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}-config.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart._chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
            ),
            CoreDNSConfigChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn coredns_config_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = CoreDNSConfigChart::new(None, false, "whatever".to_string(), "whatever".to_string());
        let chart_values_file_path = chart._chart_values_path.helm_path().clone();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            CommonChart {
                // just fake to mimic common chart for test
                chart_info: chart.chart_info,
                ..Default::default()
            },
            format!(
                "/lib/{}/bootstrap/chart_values/{}-config.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    &chart_values_file_path,
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
                ),
                CoreDNSConfigChart::chart_name(),
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
