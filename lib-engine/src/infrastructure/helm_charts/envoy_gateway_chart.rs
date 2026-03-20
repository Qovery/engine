use kube::Api;
use kube::Client;
use retry::OperationResult;
use retry::delay::Fixed;

use crate::helm::{HpaConfig, HpaMode};
use crate::infrastructure::helm_charts::{HelmChartResources, HelmChartResourcesConstraintType};
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::runtime::block_on;
use crate::services::kube_client::Gateway;
use crate::{
    errors::CommandError,
    helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces, PriorityClass},
    infrastructure::helm_charts::{
        HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart, ToHelmChartValue,
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
                    request_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                    limit_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(1000)),
                    request_memory: Some(KubernetesMemoryResourceUnit::MebiByte(256)),
                    limit_memory: Some(KubernetesMemoryResourceUnit::GibiByte(1)),
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
                    value: self.chart_resources.limit_cpu.to_helm_chart_value(),
                },
                ChartSetValue {
                    key: "deployment.envoyGateway.resources.limits.memory".to_string(),
                    value: self.chart_resources.limit_memory.to_helm_chart_value(),
                },
                ChartSetValue {
                    key: "deployment.envoyGateway.resources.requests.cpu".to_string(),
                    value: self.chart_resources.request_cpu.to_helm_chart_value(),
                },
                ChartSetValue {
                    key: "deployment.envoyGateway.resources.requests.memory".to_string(),
                    value: self.chart_resources.request_memory.to_helm_chart_value(),
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

            let mut hpa_metric_index = 0;
            if let Some(cpu_target) = config.cpu_average_utilization_percentage.as_ref() {
                chart_info.values.push(ChartSetValue {
                    key: format!("hpa.metrics[{hpa_metric_index}].type"),
                    value: "Resource".to_string(),
                });
                chart_info.values.push(ChartSetValue {
                    key: format!("hpa.metrics[{hpa_metric_index}].resource.name"),
                    value: "cpu".to_string(),
                });
                chart_info.values.push(ChartSetValue {
                    key: format!("hpa.metrics[{hpa_metric_index}].resource.target.type"),
                    value: "Utilization".to_string(),
                });
                chart_info.values.push(ChartSetValue {
                    key: format!("hpa.metrics[{hpa_metric_index}].resource.target.averageUtilization"),
                    value: cpu_target.as_u8_percent().to_string(),
                });
                hpa_metric_index += 1;
            }

            if let Some(memory_target) = config.memory_average_utilization_percentage.as_ref() {
                chart_info.values.push(ChartSetValue {
                    key: format!("hpa.metrics[{hpa_metric_index}].type"),
                    value: "Resource".to_string(),
                });
                chart_info.values.push(ChartSetValue {
                    key: format!("hpa.metrics[{hpa_metric_index}].resource.name"),
                    value: "memory".to_string(),
                });
                chart_info.values.push(ChartSetValue {
                    key: format!("hpa.metrics[{hpa_metric_index}].resource.target.type"),
                    value: "Utilization".to_string(),
                });
                chart_info.values.push(ChartSetValue {
                    key: format!("hpa.metrics[{hpa_metric_index}].resource.target.averageUtilization"),
                    value: memory_target.as_u8_percent().to_string(),
                });
            }

            // Adjust PDB
            chart_info.values.push(ChartSetValue {
                key: "podDisruptionBudget.maxUnavailable".to_string(),
                value: "20%".to_string(),
            });
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
            chart_installation_checker: Some(Box::new(EnvoyGatewayChartChecker::new(self.namespace.clone()))),
            vertical_pod_autoscaler: None,
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
pub struct EnvoyGatewayChartChecker {
    namespace: HelmChartNamespaces,
}

impl EnvoyGatewayChartChecker {
    pub fn new(namespace: HelmChartNamespaces) -> Self {
        Self { namespace }
    }

    fn has_condition_true_for_generation(gateway: &Gateway, conditions_type: &str, expected_generation: i64) -> bool {
        gateway
            .status
            .as_ref()
            .and_then(|status| status.conditions.as_ref())
            .and_then(|conditions| conditions.iter().find(|condition| condition.type_ == conditions_type))
            .map(|condition| {
                condition.status == "True" && condition.observed_generation.unwrap_or_default() >= expected_generation
            })
            .unwrap_or(false)
    }
}

impl Default for EnvoyGatewayChartChecker {
    fn default() -> Self {
        Self::new(HelmChartNamespaces::Qovery)
    }
}

impl ChartInstallationChecker for EnvoyGatewayChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        let gateway_name = "qovery-cluster-public-gateway";
        let namespace = self.namespace.to_string();
        let kube_client = kube_client.clone();

        let result = retry::retry(Fixed::from_millis(5000).take(36), || {
            // Retry every 5 seconds for up to 3 minutes.
            let gateways: Api<Gateway> = Api::namespaced(kube_client.clone(), namespace.as_str());

            let gateway = match block_on(gateways.get(gateway_name)) {
                Ok(result) => result,
                Err(e) => {
                    let err = CommandError::new(
                        format!("Error trying to get gateway (name={gateway_name}, namespace={namespace})"),
                        Some(e.to_string()),
                        None,
                    );
                    return OperationResult::Retry(err);
                }
            };

            let expected_generation = gateway.metadata.generation.unwrap_or_default();
            let is_accepted = Self::has_condition_true_for_generation(&gateway, "Accepted", expected_generation);
            let is_programmed = Self::has_condition_true_for_generation(&gateway, "Programmed", expected_generation);

            if !is_accepted || !is_programmed {
                let err = CommandError::new_from_safe_message(format!(
                    "Waiting for gateway to be accepted and programmed (name={gateway_name}, namespace={namespace}, accepted={is_accepted}, programmed={is_programmed}, generation={expected_generation})"
                ));
                return OperationResult::Retry(err);
            }

            OperationResult::Ok(())
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
