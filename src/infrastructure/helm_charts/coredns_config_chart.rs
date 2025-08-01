use crate::cmd::command::CommandKiller;
use crate::cmd::kubectl::{
    kubectl_delete_crash_looping_pods, kubectl_exec_get_configmap, kubectl_exec_rollout_restart_deployment,
    kubectl_exec_with_output, kubectl_update_crd,
};
use crate::errors::CommandError;
use crate::helm::HelmAction::Deploy;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartPayload, ChartSetValue, HelmAction, HelmChart, HelmChartError,
    HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath};
use crate::runtime::block_on;
use crate::utilities::calculate_hash;
use k8s_openapi::api::core::v1::Pod;
use kube::core::params::ListParams;
use kube::{Api, Client, ResourceExt};
use retry::OperationResult;
use retry::delay::Fixed;
use std::collections::HashMap;
use std::path::Path;

// TODO(benjaminch): refactor this chart to have only one in common (issue with labels)
#[derive(Clone)]
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
        dns_coredns_extra_config_helm_format: Option<String>,
        namespace: HelmChartNamespaces,
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

        let mut chart = CoreDNSConfigChart {
            _chart_path: chart_path.clone(),
            _chart_values_path: chart_values_path.clone(),
            chart_info: ChartInfo {
                name: CoreDNSConfigChart::chart_name(),
                path: chart_path.to_string(),
                namespace,
                action: HelmAction::Deploy,
                atomic: false,
                force_upgrade: false,
                recreate_pods: false,
                reinstall_chart_if_installed_version_is_below_than: None,
                timeout_in_seconds: 600,
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
        };
        if let Some(extra_config) = dns_coredns_extra_config_helm_format {
            chart.chart_info.values_string.push(ChartSetValue {
                key: "extra_config".to_string(),
                value: extra_config,
            });
        }
        chart
    }

    fn chart_name() -> String {
        "coredns".to_string()
    }
}

impl HelmChart for CoreDNSConfigChart {
    fn clone_dyn(&self) -> Box<dyn HelmChart> {
        Box::new(self.clone())
    }

    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    fn pre_exec(
        &self,
        kube_client: &kube::Client,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        _payload: Option<ChartPayload>,
        _cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        let kind = "configmap";
        let mut envs = envs.to_vec();
        envs.push(("KUBECONFIG", kubernetes_config.to_str().unwrap()));

        let chart_infos = self.get_chart_info();

        // Cleaning any existing crash looping pod for this helm chart
        if let Some(selector) = self.get_selector() {
            kubectl_delete_crash_looping_pods(
                kubernetes_config,
                Some(chart_infos.get_namespace_string().as_str()),
                Some(selector.as_str()),
                envs.to_vec(),
            )?;
        }

        // Force install CRDs if needed
        let chart_info = &self.get_chart_info();
        match chart_info.action {
            Deploy => {
                if let Some(crds_update) = &chart_info.crds_update {
                    if let Err(_e) =
                        kubectl_update_crd(kube_client, chart_info.name.as_str(), crds_update.path.as_str())
                    {
                        return Err(HelmChartError::CannotUpdateCrds {
                            crd_path: crds_update.path.clone(),
                        });
                    }
                }
            }
            HelmAction::Destroy => {}
        }

        // calculate current configmap checksum
        let current_configmap_hash = match kubectl_exec_get_configmap(
            kubernetes_config,
            &self.chart_info.get_namespace_string(),
            &self.chart_info.name,
            envs.to_vec(),
        ) {
            Ok(cm) => {
                if cm.data.corefile.is_none() {
                    return Err(HelmChartError::CommandError(CommandError::new_from_safe_message(
                        "Corefile data structure is not found in CoreDNS configmap".to_string(),
                    )));
                };
                calculate_hash(&cm.data.corefile.unwrap())
            }
            Err(e) => return Err(HelmChartError::CommandError(e)),
        };
        let mut configmap_hash = HashMap::new();
        configmap_hash.insert("checksum".to_string(), current_configmap_hash.to_string());
        let payload = ChartPayload::new(configmap_hash);

        // set labels and annotations to give helm ownership on coredns configmap
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
                envs.to_vec(),
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
                envs.to_vec(),
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
                envs.to_vec(),
                &mut |_| {},
                &mut |_| {},
            )?;
            Ok(())
        };
        // set labels and annotations to give helm ownership on coredns-custom configmap
        info!(
            "setting annotations and labels on {}/{}",
            &kind,
            &format!("{}-custom", &self.chart_info.name)
        );
        let steps_custom = || -> Result<(), CommandError> {
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "annotate",
                    "--overwrite",
                    kind,
                    &format!("{}-custom", &self.chart_info.name),
                    format!("meta.helm.sh/release-name={}", &self.chart_info.name).as_str(),
                ],
                envs.to_vec(),
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
                    &format!("{}-custom", &self.chart_info.name),
                    "meta.helm.sh/release-namespace=kube-system",
                ],
                envs.to_vec(),
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
                    &format!("{}-custom", &self.chart_info.name),
                    "app.kubernetes.io/managed-by=Helm",
                ],
                envs.to_vec(),
                &mut |_| {},
                &mut |_| {},
            )?;
            Ok(())
        };
        steps()?;
        // Handle errors from steps_custom gracefully
        // Best would be to execute the following only for AKS
        if let Err(e) = steps_custom() {
            warn!(
                "Failed to set annotations and labels on coredns-custom configmap: {:?}. Continuing execution...",
                e
            );
        }
        Ok(Some(payload))
    }

    fn run(
        &self,
        kube_client: &Client,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        info!("prepare and deploy chart {}", &self.get_chart_info().name);
        self.check_prerequisites()?;
        let payload = match self.pre_exec(kube_client, kubernetes_config, envs, None, cmd_killer) {
            Ok(p) => match p {
                None => {
                    return Err(HelmChartError::CommandError(CommandError::new_from_safe_message(
                        "CoreDNS configmap checksum couldn't be get, can't deploy CoreDNS chart".to_string(),
                    )));
                }
                Some(p) => p,
            },
            Err(e) => return Err(e),
        };
        if let Err(e) = self.exec(kubernetes_config, envs, None, cmd_killer) {
            error!("Error while deploying chart: {:?}", e);
            self.on_deploy_failure(kubernetes_config, envs, None)?;
            return Err(e);
        };
        self.post_exec(kube_client, kubernetes_config, envs, Some(payload), cmd_killer)?;
        Ok(None)
    }

    fn post_exec(
        &self,
        kube_client: &Client,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
        _cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        // detect configmap data change
        let previous_configmap_checksum = match &payload {
            None => {
                return Err(HelmChartError::CommandError(CommandError::new_from_safe_message(
                    "Missing payload, can't check coredns update".to_string(),
                )));
            }
            Some(x) => match x.data().get("checksum") {
                None => {
                    return Err(HelmChartError::CommandError(CommandError::new_from_safe_message(
                        "Missing configmap checksum, can't check coredns diff".to_string(),
                    )));
                }
                Some(c) => c.clone(),
            },
        };
        let current_configmap_checksum = match kubectl_exec_get_configmap(
            kubernetes_config,
            &self.chart_info.get_namespace_string(),
            &self.chart_info.name,
            envs.to_vec(),
        ) {
            Ok(cm) => {
                if cm.data.corefile.is_none() {
                    return Err(HelmChartError::CommandError(CommandError::new_from_safe_message(
                        "Corefile data structure is not found in CoreDNS configmap".to_string(),
                    )));
                };
                calculate_hash(&cm.data.corefile.unwrap()).to_string()
            }
            Err(e) => return Err(HelmChartError::CommandError(e)),
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
            envs,
        )?;

        self.chart_installation_checker.verify_installation(kube_client)?;

        Ok(None)
    }
}

#[derive(Clone)]
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

        let result = retry::retry(Fixed::from_millis(10000).take(12), || {
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
                                &coredns_pod.name_any(),
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
            Err(retry::Error { error, .. }) => Err(error),
        }
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{CommonChart, HelmChartNamespaces};
    use crate::infrastructure::helm_charts::coredns_config_chart::CoreDNSConfigChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn coredns_config_chart_directory_exists_test() {
        // setup:
        let chart = CoreDNSConfigChart::new(
            None,
            false,
            "whatever".to_string(),
            "whatever".to_string(),
            Some("whatever".to_string()),
            HelmChartNamespaces::KubeSystem,
        );

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
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn coredns_config_chart_values_file_exists_test() {
        // setup:
        let chart = CoreDNSConfigChart::new(
            None,
            false,
            "whatever".to_string(),
            "whatever".to_string(),
            Some("whatever".to_string()),
            HelmChartNamespaces::KubeSystem,
        );

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
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn coredns_config_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = CoreDNSConfigChart::new(
            None,
            false,
            "whatever".to_string(),
            "whatever".to_string(),
            Some("whatever".to_string()),
            HelmChartNamespaces::KubeSystem,
        );
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
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }
}
