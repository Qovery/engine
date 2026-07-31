use kube::Api;
use kube::Client;
use retry::OperationResult;
use retry::delay::Fixed;
use std::time::Duration;

use crate::engine_task::qovery_api::{GatewayConditionEntry, GatewayStatus, SharedClusterFailureContext};
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
    pub replicas: u32,
}

impl Default for EnvoyGatewayOptions {
    fn default() -> Self {
        Self { replicas: 1 }
    }
}

pub struct EnvoyGatewayChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    priority_class: PriorityClass,
    chart_resources: HelmChartResources,
    options: EnvoyGatewayOptions,
    cluster_failure_context: SharedClusterFailureContext,
}

impl EnvoyGatewayChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_values_location: HelmChartDirectoryLocation,
        namespace: HelmChartNamespaces,
        priority_class: PriorityClass,
        chart_resources_constraint_type: HelmChartResourcesConstraintType,
        options: EnvoyGatewayOptions,
        cluster_failure_context: SharedClusterFailureContext,
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
            cluster_failure_context,
        }
    }

    pub fn chart_name() -> String {
        "envoy-gateway".to_string()
    }
}

impl ToCommonHelmChart for EnvoyGatewayChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, crate::helm::HelmChartError> {
        let chart_timeout = Duration::from_secs(60 * 20);
        let mut chart_info = ChartInfo {
            name: EnvoyGatewayChart::chart_name(),
            path: self.chart_path.to_string(),
            namespace: self.namespace.clone(),
            values_files: vec![self.chart_values_path.to_string()],
            // Gateway API and Envoy Gateway CRDs are installed by the dedicated CRD chart first.
            // Passing --skip-crds here prevents the controller chart from reapplying bundled CRDs,
            // which GKE rejects when it enforces the standard Gateway API channel.
            skip_crds: true,
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
            // Because of ALB, svc can take some time to start
            // rolling out the deployment can take a lot of time based on provider, region and cluster size, so we set a long timeout to avoid killing the deployment too early
            timeout_in_seconds: chart_timeout.as_secs() as i64,
            ..Default::default()
        };

        // Set custom priority class if provided
        if let PriorityClass::Qovery(priority_class) = &self.priority_class {
            chart_info.values.push(ChartSetValue {
                key: "priorityClassName".to_string(),
                value: priority_class.to_string(),
            });
        }

        chart_info.values.push(ChartSetValue {
            key: "hpa.enabled".to_string(),
            value: false.to_string(),
        });
        chart_info.values.push(ChartSetValue {
            key: "deployment.replicas".to_string(),
            value: self.options.replicas.to_string(),
        });
        // Keep the fixed-replica invariants explicit in Rust instead of relying on chart defaults.
        chart_info.values.push(ChartSetValue {
            key: "podDisruptionBudget.maxUnavailable".to_string(),
            value: 1.to_string(),
        });

        Ok(CommonChart {
            chart_info,
            chart_installation_checker: Some(Box::new(EnvoyGatewayChartChecker::new(
                self.namespace.clone(),
                Duration::from_secs(60 * 10),
                self.cluster_failure_context.clone(),
            ))),
            vertical_pod_autoscaler: None,
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
pub struct EnvoyGatewayChartChecker {
    namespace: HelmChartNamespaces,
    readiness_timeout: Duration,
    cluster_failure_context: SharedClusterFailureContext,
}

impl EnvoyGatewayChartChecker {
    const RETRY_INTERVAL: Duration = Duration::from_secs(5);

    pub fn new(
        namespace: HelmChartNamespaces,
        readiness_timeout: Duration,
        cluster_failure_context: SharedClusterFailureContext,
    ) -> Self {
        Self {
            namespace,
            readiness_timeout,
            cluster_failure_context,
        }
    }

    fn retry_attempts_for_timeout(timeout: Duration) -> usize {
        timeout.as_millis().div_ceil(Self::RETRY_INTERVAL.as_millis()).max(1) as usize
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

impl ChartInstallationChecker for EnvoyGatewayChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        let gateway_name = "qovery-cluster-public-gateway";
        let namespace = self.namespace.to_string();
        let kube_client = kube_client.clone();
        let cluster_failure_context = self.cluster_failure_context.clone();

        let result = retry::retry(
            Fixed::from_millis(Self::RETRY_INTERVAL.as_millis() as u64)
                .take(Self::retry_attempts_for_timeout(self.readiness_timeout)),
            || {
                // Give Gateway API conditions their own bounded window after Helm reports success.
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
                let is_programmed =
                    Self::has_condition_true_for_generation(&gateway, "Programmed", expected_generation);

                if !is_accepted || !is_programmed {
                    let conditions: Vec<GatewayConditionEntry> = gateway
                        .status
                        .as_ref()
                        .and_then(|s| s.conditions.as_ref())
                        .map(|conds| {
                            conds
                                .iter()
                                .filter(|c| c.type_ == "Accepted" || c.type_ == "Programmed")
                                .map(|c| GatewayConditionEntry {
                                    type_: c.type_.clone(),
                                    status: c.status.clone(),
                                    reason: c.reason.clone(),
                                    message: c.message.clone(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    let condition_details = conditions
                        .iter()
                        .map(|c| {
                            format!(
                                "{}(status={}, reason={}, message={})",
                                c.type_,
                                c.status,
                                c.reason.as_deref().unwrap_or(""),
                                c.message.as_deref().unwrap_or(""),
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");

                    cluster_failure_context.lock().gateway_status = GatewayStatus {
                        gateway_name: gateway_name.to_string(),
                        conditions,
                    };

                    let err = CommandError::new_from_safe_message(format!(
                        "Waiting for gateway to be accepted and programmed (name={gateway_name}, namespace={namespace}, accepted={is_accepted}, programmed={is_programmed}, generation={expected_generation}, conditions=[{condition_details}])"
                    ));
                    return OperationResult::Retry(err);
                }

                OperationResult::Ok(())
            },
        );

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
    use crate::engine_task::qovery_api::SharedClusterFailureContext;
    use crate::helm::{HelmChartNamespaces, PriorityClass};
    use crate::infrastructure::helm_charts::envoy_gateway_chart::{
        EnvoyGatewayChart, EnvoyGatewayChartChecker, EnvoyGatewayOptions,
    };
    use crate::infrastructure::helm_charts::{
        HelmChartDirectoryLocation, HelmChartResourcesConstraintType, HelmChartType, ToCommonHelmChart,
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use std::env;
    use std::time::Duration;
    use uuid::Uuid;

    fn fake_failure_context() -> SharedClusterFailureContext {
        SharedClusterFailureContext::new(Uuid::nil(), None)
    }

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
            fake_failure_context(),
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
            fake_failure_context(),
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
            fake_failure_context(),
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

    #[test]
    fn envoy_gateway_checker_retry_budget_matches_chart_timeout() {
        let chart = EnvoyGatewayChart::new(
            None,
            HelmChartDirectoryLocation::CommonFolder,
            HelmChartNamespaces::Qovery,
            PriorityClass::Default,
            HelmChartResourcesConstraintType::ChartDefault,
            EnvoyGatewayOptions::default(),
            fake_failure_context(),
        );
        let common_chart = chart.to_common_helm_chart().unwrap();
        assert_eq!(
            EnvoyGatewayChartChecker::retry_attempts_for_timeout(Duration::from_secs(
                common_chart.chart_info.timeout_in_seconds as u64,
            )),
            240
        );
    }

    #[test]
    fn envoy_gateway_checker_retry_budget_matches_checker_timeout() {
        assert_eq!(
            EnvoyGatewayChartChecker::retry_attempts_for_timeout(Duration::from_secs(10 * 60)),
            120
        );
    }

    #[test]
    fn envoy_gateway_chart_sets_fixed_replicas() {
        let chart = EnvoyGatewayChart::new(
            None,
            HelmChartDirectoryLocation::CommonFolder,
            HelmChartNamespaces::Qovery,
            PriorityClass::Default,
            HelmChartResourcesConstraintType::ChartDefault,
            EnvoyGatewayOptions { replicas: 3 },
            fake_failure_context(),
        );

        let common_chart = chart.to_common_helm_chart().unwrap();

        assert!(
            common_chart
                .chart_info
                .values
                .iter()
                .any(|value| value.key == "hpa.enabled" && value.value == "false")
        );
        assert!(
            common_chart
                .chart_info
                .values
                .iter()
                .any(|value| value.key == "deployment.replicas" && value.value == "3")
        );
        assert!(
            common_chart
                .chart_info
                .values
                .iter()
                .any(|value| value.key == "podDisruptionBudget.maxUnavailable" && value.value == "1")
        );
    }
}
