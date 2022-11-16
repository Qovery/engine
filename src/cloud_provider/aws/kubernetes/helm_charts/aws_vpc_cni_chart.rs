use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartPayload, ChartSetValue, HelmChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath};
use crate::cmd::kubectl::{kubectl_delete_crash_looping_pods, kubectl_exec_get_daemonset, kubectl_exec_with_output};
use crate::errors::{CommandError, ErrorMessageVerbosity};
use crate::runtime::block_on;
use k8s_openapi::api::apps::v1::DaemonSet;
use kube::core::params::ListParams;
use kube::{Api, Client};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

// TODO(benjaminch): This chart to be migrated to kube.rs
pub struct AwsVpcCniChart {
    chart_info: ChartInfo,
    _chart_path: HelmChartPath,
    _chart_values_path: HelmChartValuesFilePath,
    chart_installation_checker: AwsVpcCniChartInstallationChecker,
}

impl AwsVpcCniChart {
    pub fn new(
        version: String,
        chart_prefix_path: Option<&str>,
        chart_image_region: String,
        chart_should_support_original_match_labels: bool,
        cluster_name: String,
    ) -> AwsVpcCniChart {
        let chart_path = HelmChartPath::new(
            chart_prefix_path,
            HelmChartDirectoryLocation::CloudProviderFolder,
            AwsVpcCniChart::chart_name(),
        );
        let chart_values_path = HelmChartValuesFilePath::new(
            chart_prefix_path,
            HelmChartDirectoryLocation::CloudProviderFolder,
            AwsVpcCniChart::chart_name(),
        );

        AwsVpcCniChart {
            _chart_path: chart_path.clone(),
            _chart_values_path: chart_values_path.clone(),
            chart_info: ChartInfo {
                name: AwsVpcCniChart::chart_name(),
                path: chart_path.to_string(),
                namespace: HelmChartNamespaces::KubeSystem,
                values_files: vec![chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "image.region".to_string(),
                        value: chart_image_region.to_string(),
                    },
                    ChartSetValue {
                        key: "init.image.region".to_string(),
                        value: chart_image_region,
                    },
                    // this is required to know if we need to keep old annotation/labels values or not
                    ChartSetValue {
                        key: "originalMatchLabels".to_string(),
                        value: chart_should_support_original_match_labels.to_string(),
                    },
                    // label ENIs
                    ChartSetValue {
                        key: "env.CLUSTER_NAME".to_string(),
                        value: cluster_name,
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: AwsVpcCniChartInstallationChecker::new(version),
        }
    }

    fn chart_name() -> String {
        "aws-vpc-cni".to_string()
    }

    fn enable_cni_managed_by_helm(&self, kubernetes_config: &Path, envs: &[(String, String)]) -> bool {
        let environment_variables = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();

        match kubectl_exec_get_daemonset(
            kubernetes_config,
            self.chart_info.name.to_string().as_str(),
            self.chart_info.namespace.to_string().as_str(),
            Some("k8s-app=aws-node,app.kubernetes.io/managed-by=Helm"),
            environment_variables,
        ) {
            Ok(x) => x.items.is_some() && x.items.unwrap().is_empty(),
            Err(e) => {
                error!(
                    "error while getting daemonset info for chart {}, won't deploy CNI chart. {:?}",
                    self.chart_info.name.to_string(),
                    e
                );
                false
            }
        }
    }
}

impl HelmChart for AwsVpcCniChart {
    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    // TODO(benjaminch): This piece of code should be handled via a dedicated struct, no need to override here.
    fn pre_exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        _payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        let kinds = vec!["daemonSet", "clusterRole", "clusterRoleBinding", "serviceAccount"];
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

        match self.enable_cni_managed_by_helm(kubernetes_config, envs) {
            true => {
                for kind in kinds {
                    // Setting annotations and labels on kind/aws-node
                    let steps = || -> Result<(), CommandError> {
                        let label = format!("meta.helm.sh/release-name={}", self.chart_info.name);
                        let args = vec![
                            "-n",
                            "kube-system",
                            "annotate",
                            "--overwrite",
                            kind,
                            "aws-node",
                            label.as_str(),
                        ];
                        let mut stdout = "".to_string();
                        let mut stderr = "".to_string();

                        kubectl_exec_with_output(
                            args.clone(),
                            environment_variables.clone(),
                            &mut |out| stdout = format!("{}\n{}", stdout, out),
                            &mut |out| stderr = format!("{}\n{}", stderr, out),
                        )?;

                        let args = vec![
                            "-n",
                            "kube-system",
                            "annotate",
                            "--overwrite",
                            kind,
                            "aws-node",
                            "meta.helm.sh/release-namespace=kube-system",
                        ];
                        let mut stdout = "".to_string();
                        let mut stderr = "".to_string();

                        kubectl_exec_with_output(
                            args.clone(),
                            environment_variables.clone(),
                            &mut |out| stdout = format!("{}\n{}", stdout, out),
                            &mut |out| stderr = format!("{}\n{}", stderr, out),
                        )?;

                        let args = vec![
                            "-n",
                            "kube-system",
                            "label",
                            "--overwrite",
                            kind,
                            "aws-node",
                            "app.kubernetes.io/managed-by=Helm",
                        ];
                        let mut stdout = "".to_string();
                        let mut stderr = "".to_string();

                        kubectl_exec_with_output(
                            args.clone(),
                            environment_variables.clone(),
                            &mut |out| stdout = format!("{}\n{}", stdout, out),
                            &mut |out| stderr = format!("{}\n{}", stderr, out),
                        )?;

                        Ok(())
                    };

                    steps()?;
                }

                // sleep in order to be sure the daemonset is updated
                sleep(Duration::from_secs(30))
            }
            false => {} // AWS CNI is already supported by Helm, nothing to do
        };

        Ok(None)
    }

    fn post_exec(
        &self,
        kube_client: &kube::Client,
        _kubernetes_config: &Path,
        _envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        match self.chart_installation_checker.verify_installation(kube_client) {
            Ok(_) => Ok(payload),
            Err(e) => Err(e),
        }
    }
}

pub struct AwsVpcCniChartInstallationChecker {
    aws_vpc_cni_chart_version: String,
}

impl AwsVpcCniChartInstallationChecker {
    pub fn new(aws_vpc_cni_chart_version: String) -> Self {
        AwsVpcCniChartInstallationChecker {
            aws_vpc_cni_chart_version,
        }
    }
}

impl ChartInstallationChecker for AwsVpcCniChartInstallationChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        // This is a verify basic check: make sure CNI daemon is running and has current chart version set
        let cni_daemonset: Api<DaemonSet> = Api::all(kube_client.clone());

        match block_on(
            cni_daemonset.list(
                &ListParams::default().labels(
                    format!(
                        "helm.sh/chart={}-{}",
                        AwsVpcCniChart::chart_name(),
                        self.aws_vpc_cni_chart_version
                    )
                    .as_str(),
                ),
            ),
        ) {
            Ok(cni_daemonset_result) => {
                if cni_daemonset_result.items.is_empty() {
                    return Err(CommandError::new_from_safe_message(format!(
                        "Error: {} version {} is not installed",
                        AwsVpcCniChart::chart_name(),
                        self.aws_vpc_cni_chart_version,
                    )));
                }
            }
            Err(e) => {
                return Err(CommandError::new(
                    format!(
                        "Error trying to get daemonset {} version {}",
                        AwsVpcCniChart::chart_name(),
                        self.aws_vpc_cni_chart_version,
                    ),
                    Some(e.to_string()),
                    None,
                ))
            }
        }

        // TODO(benjaminch): Check properly if CNI is working, probably via exporters

        Ok(())
    }
}

// TODO(benjaminch): this function should be handled otherwise using kube.rs
pub fn is_cni_old_version_installed(
    kubernetes_config: &Path,
    envs: &[(String, String)],
    namespace: HelmChartNamespaces,
) -> Result<bool, CommandError> {
    let name = "aws-node";
    let environment_variables = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();

    match kubectl_exec_get_daemonset(
        kubernetes_config,
        name,
        namespace.to_string().as_str(),
        None,
        environment_variables,
    ) {
        Ok(x) => match x.spec {
            None => Err(CommandError::new_from_safe_message(format!(
                "Spec was not found in json output while looking at daemonset {} in {}.",
                name, namespace
            ))),
            Some(spec) => match spec.selector.match_labels.k8s_app {
                Some(x) if x == name => Ok(true),
                _ => Ok(false),
            },
        },
        Err(e) => Err(CommandError::new(
            format!(
                "Error while getting daemonset info for chart {} in {}. {}",
                name,
                namespace,
                e.message(ErrorMessageVerbosity::SafeOnly)
            ),
            e.message_raw(),
            e.env_vars(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::helm_charts::aws_vpc_cni_chart::AwsVpcCniChart;
    use crate::cloud_provider::helm::CommonChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn aws_vpc_cni_chart_directory_exists_test() {
        // setup:
        let chart = AwsVpcCniChart::new(
            "whatever".to_string(),
            None,
            "whatever".to_string(),
            true,
            "whatever".to_string(),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart._chart_path.helm_path(), Some(KubernetesKind::Eks)),
            AwsVpcCniChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }

    /// Makes sure chart values file exists.
    #[test]
    fn aws_vpc_cni_chart_values_file_exists_test() {
        // setup:
        let chart = AwsVpcCniChart::new(
            "whatever".to_string(),
            None,
            "whatever".to_string(),
            true,
            "whatever".to_string(),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart._chart_values_path.helm_path(),
                Some(KubernetesKind::Eks)
            ),
            AwsVpcCniChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn aws_vpc_cni_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = AwsVpcCniChart::new(
            "whatever".to_string(),
            None,
            "whatever".to_string(),
            true,
            "whatever".to_string(),
        );

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            CommonChart {
                // just fake to mimic common chart for test
                chart_info: chart.chart_info,
                ..Default::default()
            },
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart._chart_values_path.helm_path(),
                    Some(KubernetesKind::Eks)
                ),
                AwsVpcCniChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
