use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartPayload, ChartSetValue, HelmAction, HelmChart, HelmChartNamespaces,
};
use crate::cmd::kubectl::{
    kubectl_delete_crash_looping_pods, kubectl_exec_get_configmap, kubectl_exec_rollout_restart_deployment,
    kubectl_exec_with_output,
};
use crate::errors::{CommandError, ErrorMessageVerbosity};
use crate::utilities::calculate_hash;
use itertools::Itertools;
use kube::Client;
use std::collections::HashMap;
use std::path::Path;

// TODO(benjaminch): This chart should be factorized accross all providers.
pub struct CoreDNSConfigChart {
    pub chart_info: ChartInfo,
    chart_installation_checker: CoreDNSConfigChartChecker,
}

impl CoreDNSConfigChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        custom_labels: Vec<String>,
        declare_hosts: bool,
        managed_dns_helm_format: String,
        managed_dns_resolvers_terraform_format: String,
    ) -> CoreDNSConfigChart {
        CoreDNSConfigChart {
            chart_info: ChartInfo {
                name: CoreDNSConfigChart::chart_name(),
                path: format!(
                    "{}/common/charts/{}-config",
                    chart_prefix_path.unwrap_or("./"),
                    CoreDNSConfigChart::chart_name()
                ),
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
                values: vec![
                    ChartSetValue {
                        key: "labels".to_string(),
                        value: format!("{{{}}}", custom_labels.iter().join(",")),
                    },
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
                values_string: vec![],
                values_files: vec![],
                yaml_files_content: vec![],
                parse_stderr_for_error: false,
                k8s_selector: None,
                backup_resources: None,
                crds_update: None,
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
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1367): Implement chart install verification
        Ok(())
    }
}
