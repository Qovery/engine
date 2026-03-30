use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{Api, Client};
use retry::{OperationResult, delay::Fixed};
use std::path::Path;

use crate::helm::ChartSetValue;
use crate::runtime::block_on;
use crate::{
    errors::CommandError,
    helm::{ChartInfo, ChartInstallationChecker, ChartPreExecuteAction, CommonChart},
    infrastructure::helm_charts::{
        HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
    },
};

#[derive(Clone)]
pub struct EnvoyGatewayCrdChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    include_gateway_api_crds: bool,
    include_envoy_proxy_crds: bool,
}

impl EnvoyGatewayCrdChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_values_location: HelmChartDirectoryLocation,
        include_gateway_api_crds: bool,
        include_envoy_proxy_crds: bool,
    ) -> Self {
        Self {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                EnvoyGatewayCrdChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                chart_values_location,
                EnvoyGatewayCrdChart::chart_name(),
            ),
            include_envoy_proxy_crds,
            include_gateway_api_crds,
        }
    }

    pub fn chart_name() -> String {
        "envoy-gateway-crd".to_string()
    }
}

impl ToCommonHelmChart for EnvoyGatewayCrdChart {
    fn to_common_helm_chart(&self) -> Result<crate::helm::CommonChart, crate::helm::HelmChartError> {
        let chart_info = ChartInfo {
            name: EnvoyGatewayCrdChart::chart_name(),
            path: self.chart_path.to_string(),
            values_files: vec![self.chart_values_path.to_string()],
            values: vec![
                ChartSetValue {
                    key: "crds.gatewayAPI.enabled".to_string(),
                    value: self.include_gateway_api_crds.to_string(),
                },
                ChartSetValue {
                    key: "crds.envoyGateway.enabled".to_string(),
                    value: self.include_envoy_proxy_crds.to_string(),
                },
            ],
            force_conflicts: true, // CRDs may already exist from previous installations, and we want to ensure they are updated if needed.
            requires_server_side_apply: true,
            ..Default::default()
        };

        Ok(CommonChart {
            chart_info,
            chart_installation_checker: Some(Box::new(EnvoyGatewayCrdChartChecker::new())),
            vertical_pod_autoscaler: None,
            pre_execute_action: Some(Box::new(RemoveGatewayApiValidatingAdmissionPolicyAction)),
        })
    }
}

#[derive(Clone)]
pub struct EnvoyGatewayCrdChartChecker {}

impl EnvoyGatewayCrdChartChecker {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for EnvoyGatewayCrdChartChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartInstallationChecker for EnvoyGatewayCrdChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());

        let required_crds = [
            "gatewayclasses.gateway.networking.k8s.io",
            "gateways.gateway.networking.k8s.io",
            "httproutes.gateway.networking.k8s.io",
        ];

        let envoy_crds = [
            "envoyproxies.gateway.envoyproxy.io",
            "backendtrafficpolicies.gateway.envoyproxy.io",
            "securitypolicies.gateway.envoyproxy.io",
        ];

        let result = retry::retry(Fixed::from_millis(10_000).take(6), || {
            for crd_name in &required_crds {
                match block_on(crds.get(crd_name)) {
                    Ok(crd) => {
                        let is_established = crd
                            .status
                            .as_ref()
                            .and_then(|s| s.conditions.as_ref())
                            .map(|conditions| {
                                conditions
                                    .iter()
                                    .any(|c| c.type_ == "Established" && c.status == "True")
                            })
                            .unwrap_or(false);

                        if !is_established {
                            return OperationResult::Retry(CommandError::new_from_safe_message(format!(
                                "Gateway API CRD '{crd_name}' exists but is not yet established"
                            )));
                        }
                    }
                    Err(e) => {
                        return OperationResult::Retry(CommandError::new_from_safe_message(format!(
                            "Gateway API CRD '{crd_name}' not found: {e}"
                        )));
                    }
                }
            }

            for crd_name in &envoy_crds {
                match block_on(crds.get(crd_name)) {
                    Ok(crd) => {
                        let is_established = crd
                            .status
                            .as_ref()
                            .and_then(|s| s.conditions.as_ref())
                            .map(|conditions| {
                                conditions
                                    .iter()
                                    .any(|c| c.type_ == "Established" && c.status == "True")
                            })
                            .unwrap_or(false);

                        if !is_established {
                            return OperationResult::Retry(CommandError::new_from_safe_message(format!(
                                "Envoy Gateway CRD '{crd_name}' exists but is not yet established"
                            )));
                        }
                    }
                    Err(e) => {
                        return OperationResult::Retry(CommandError::new_from_safe_message(format!(
                            "Envoy Gateway CRD '{crd_name}' not found: {e}"
                        )));
                    }
                }
            }

            OperationResult::Ok(())
        });

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e.error),
        }
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct RemoveGatewayApiValidatingAdmissionPolicyAction;

impl ChartPreExecuteAction for RemoveGatewayApiValidatingAdmissionPolicyAction {
    fn execute(&self, kubernetes_config: &Path, envs: Vec<(&str, &str)>) -> Result<(), CommandError> {
        use crate::cmd::kubectl::kubectl_delete_validating_admission_policy;

        // `kubectl_delete_validating_admission_policy` takes `envs` by value; clone the vector
        // to reuse it for the second deletion call.
        kubectl_delete_validating_admission_policy(
            kubernetes_config,
            "safe-upgrades.gateway.networking.k8s.io",
            envs.clone(),
        )?;
        kubectl_delete_validating_admission_policy(kubernetes_config, "enforce-gateway-standard-channel", envs)?;

        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartPreExecuteAction> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::helm_charts::envoy_gateway_crd_chart::EnvoyGatewayCrdChart;
    use crate::infrastructure::helm_charts::{
        HelmChartDirectoryLocation, HelmChartType, ToCommonHelmChart,
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn envoy_gateway_crd_chart_directory_exists_test() {
        // setup:
        let chart = EnvoyGatewayCrdChart::new(None, HelmChartDirectoryLocation::CommonFolder, true, true);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            EnvoyGatewayCrdChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn envoy_gateway_crd_chart_values_file_exists_test() {
        // setup:
        let chart = EnvoyGatewayCrdChart::new(None, HelmChartDirectoryLocation::CommonFolder, true, true);

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
            EnvoyGatewayCrdChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn envoy_gateway_crd_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = EnvoyGatewayCrdChart::new(None, HelmChartDirectoryLocation::CommonFolder, true, true);
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
                EnvoyGatewayCrdChart::chart_name()
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
