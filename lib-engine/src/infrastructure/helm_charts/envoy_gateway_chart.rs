use kube::Client;

use crate::helm::{HpaConfig, HpaMode};
use crate::infrastructure::helm_charts::{HelmChartResources, HelmChartResourcesConstraintType};
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::{
    errors::CommandError,
    helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces, PriorityClass},
    infrastructure::helm_charts::{
        HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
    },
};

pub struct EnvoyGatewayOptions {
    pub hpa_mode: HpaMode,
}

impl Default for EnvoyGatewayOptions {
    fn default() -> Self {
        Self {
            hpa_mode: HpaMode::Enabled {
                config: HpaConfig::default(),
            },
        }
    }
}

pub struct EnvoyGatewayChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    priority_class: PriorityClass,
    chart_resources: HelmChartResources,
    options: EnvoyGatewayOptions,
}

impl EnvoyGatewayChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_values_location: HelmChartDirectoryLocation,
        namespace: HelmChartNamespaces,
        priority_class: PriorityClass,
        chart_resources_constraint_type: HelmChartResourcesConstraintType,
        options: EnvoyGatewayOptions,
    ) -> Self {
        Self {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                EnvoyGatewayChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                chart_values_location,
                EnvoyGatewayChart::chart_name(),
            ),
            namespace,
            priority_class,
            chart_resources: match chart_resources_constraint_type {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(256),
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            options,
        }
    }

    pub fn chart_name() -> String {
        "envoy-gateway".to_string()
    }
}

impl ToCommonHelmChart for EnvoyGatewayChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, crate::helm::HelmChartError> {
        let mut chart_info = ChartInfo {
            name: EnvoyGatewayChart::chart_name(),
            path: self.chart_path.to_string(),
            namespace: self.namespace.clone(),
            values_files: vec![self.chart_values_path.to_string()],
            values: vec![
                // resources limits
                ChartSetValue {
                    key: "deployment.envoyGateway.resources.limits.cpu".to_string(),
                    value: self.chart_resources.limit_cpu.to_string(),
                },
                ChartSetValue {
                    key: "deployment.envoyGateway.resources.limits.memory".to_string(),
                    value: self.chart_resources.limit_memory.to_string(),
                },
                ChartSetValue {
                    key: "deployment.envoyGateway.resources.requests.cpu".to_string(),
                    value: self.chart_resources.request_cpu.to_string(),
                },
                ChartSetValue {
                    key: "deployment.envoyGateway.resources.requests.memory".to_string(),
                    value: self.chart_resources.request_memory.to_string(),
                },
            ],
            ..Default::default()
        };

        // Set custom priority class if provided
        if let PriorityClass::Qovery(priority_class) = &self.priority_class {
            chart_info.values.push(ChartSetValue {
                key: "priorityClassName".to_string(),
                value: priority_class.to_string(),
            });
        }

        // Set HPA mode
        if let HpaMode::Enabled { config } = &self.options.hpa_mode {
            chart_info.values.push(ChartSetValue {
                key: "hpa.enabled".to_string(),
                value: "true".to_string(),
            });
            chart_info.values.push(ChartSetValue {
                key: "hpa.minReplicas".to_string(),
                value: config.min_replicas.to_string(),
            });
            chart_info.values.push(ChartSetValue {
                key: "hpa.maxReplicas".to_string(),
                value: config.max_replicas.to_string(),
            });
            // Adjust PDB
            chart_info.values.push(ChartSetValue {
                key: "podDisruptionBudget.maxUnavailable".to_string(),
                value: "20%".to_string(),
            });

            // TODO(benjaminch): Handle HPA CPU / Memory thresholds when needed.
        } else {
            chart_info.values.push(ChartSetValue {
                key: "hpa.enabled".to_string(),
                value: "false".to_string(),
            });
            // Adjust PDB
            chart_info.values.push(ChartSetValue {
                key: "podDisruptionBudget.maxUnavailable".to_string(),
                value: 1.to_string(),
            });
        }

        Ok(CommonChart {
            chart_info,
            chart_installation_checker: Some(Box::new(EnvoyGatewayChartChecker::new())),
            ..Default::default()
        })
    }
}

#[derive(Clone)]
pub struct EnvoyGatewayChartChecker {}

impl EnvoyGatewayChartChecker {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for EnvoyGatewayChartChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartInstallationChecker for EnvoyGatewayChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(benjaminch): Implement actual verification logic for Envoy Gateway chart installation.
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmChartNamespaces, PriorityClass};
    use crate::infrastructure::helm_charts::envoy_gateway_chart::{EnvoyGatewayChart, EnvoyGatewayOptions};
    use crate::infrastructure::helm_charts::{
        HelmChartDirectoryLocation, HelmChartResourcesConstraintType, HelmChartType, ToCommonHelmChart,
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn envoy_gateway_chart_directory_exists_test() {
        // setup:
        let chart = EnvoyGatewayChart::new(
            None,
            HelmChartDirectoryLocation::CommonFolder,
            HelmChartNamespaces::Qovery,
            PriorityClass::Default,
            HelmChartResourcesConstraintType::ChartDefault,
            EnvoyGatewayOptions::default(),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            EnvoyGatewayChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn envoy_gateway_chart_values_file_exists_test() {
        // setup:
        let chart = EnvoyGatewayChart::new(
            None,
            HelmChartDirectoryLocation::CommonFolder,
            HelmChartNamespaces::Qovery,
            PriorityClass::Default,
            HelmChartResourcesConstraintType::ChartDefault,
            EnvoyGatewayOptions::default(),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared,
            ),
            EnvoyGatewayChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn envoy_gateway_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = EnvoyGatewayChart::new(
            None,
            HelmChartDirectoryLocation::CommonFolder,
            HelmChartNamespaces::Qovery,
            PriorityClass::Default,
            HelmChartResourcesConstraintType::ChartDefault,
            EnvoyGatewayOptions::default(),
        );
        let mut common_chart = chart.to_common_helm_chart().unwrap();

        // Filter out extraArgs.* values since extraArgs is an empty object {} in the YAML
        // and we dynamically set individual keys like extraArgs.cloudflare-proxied
        common_chart
            .chart_info
            .values
            .retain(|value| !value.key.starts_with("extraArgs."));

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared,
                ),
                EnvoyGatewayChart::chart_name()
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
