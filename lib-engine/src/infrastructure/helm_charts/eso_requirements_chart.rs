use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInfoUpgradeRetry, ChartInstallationChecker, CommonChart, HelmAction, HelmChartError,
    HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::runtime::block_on;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::Client;
use kube::api::Api;
use retry::delay::Fixed;
use retry::{OperationResult, retry};
use tracing::info;

pub struct EsoRequirementsChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    action: HelmAction,
    namespace: HelmChartNamespaces,
}

impl EsoRequirementsChart {
    pub fn new(chart_prefix_path: Option<&str>, action: HelmAction, namespace: HelmChartNamespaces) -> Self {
        EsoRequirementsChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                EsoRequirementsChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                EsoRequirementsChart::chart_name(),
            ),
            action,
            namespace,
        }
    }

    pub fn chart_name() -> String {
        "external-secrets-requirements".to_string()
    }
}

impl ToCommonHelmChart for EsoRequirementsChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: EsoRequirementsChart::chart_name(),
                action: self.action.clone(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                upgrade_retry: Some(ChartInfoUpgradeRetry {
                    nb_retry: 5,
                    delay_in_milli_sec: 3_000,
                }),
                ..Default::default()
            },
            chart_installation_checker: match self.action {
                HelmAction::Deploy => Some(Box::new(EsoRequirementsChartChecker::new())),
                HelmAction::Destroy => None,
            },
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct EsoRequirementsChartChecker {}

impl EsoRequirementsChartChecker {
    pub fn new() -> Self {
        EsoRequirementsChartChecker {}
    }
}

impl Default for EsoRequirementsChartChecker {
    fn default() -> Self {
        EsoRequirementsChartChecker::new()
    }
}

impl ChartInstallationChecker for EsoRequirementsChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());

        let required_crds = [
            "clusterexternalsecrets.external-secrets.io",
            "clustergenerators.external-secrets.io",
            "clusterpushsecrets.external-secrets.io",
            "clustersecretstores.external-secrets.io",
            "externalsecrets.external-secrets.io",
            "generatorstates.external-secrets.io",
            "pushsecrets.external-secrets.io",
            "secretstores.external-secrets.io",
        ];

        // Retry: 6 attempts x 10 seconds = 1 minute max
        let result = retry(Fixed::from_millis(10_000).take(6), || {
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
                                "CRD '{crd_name}' exists but is not yet established"
                            )));
                        }
                    }
                    Err(e) => {
                        return OperationResult::Retry(CommandError::new_from_safe_message(format!(
                            "CRD '{crd_name}' not found: {e}"
                        )));
                    }
                }
            }
            OperationResult::Ok(())
        });

        match result {
            Ok(_) => {
                info!("Successfully verified all ESO CRDs are installed and established");
                Ok(())
            }
            Err(retry::Error { error, .. }) => Err(error),
        }
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::helm_charts::{HelmChartType, get_helm_path_kubernetes_provider_sub_folder_name};
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn eso_requirements_chart_directory_exists_test() {
        // setup:
        let chart = EsoRequirementsChart::new(None, HelmAction::Deploy, HelmChartNamespaces::KubeSystem);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            EsoRequirementsChart::chart_name(),
        );

        // execute
        let chart_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(chart_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn eso_requirements_chart_values_file_exists_test() {
        // setup:
        let chart = EsoRequirementsChart::new(None, HelmAction::Deploy, HelmChartNamespaces::KubeSystem);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared
            ),
            EsoRequirementsChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&values_path);

        // verify:
        assert!(values_file.is_ok(), "Values file should exist: `{values_path}`");
    }

    /// Makes sure CRD template files exist.
    #[test]
    fn eso_requirements_crd_templates_exist_test() {
        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart = EsoRequirementsChart::new(None, HelmAction::Deploy, HelmChartNamespaces::KubeSystem);
        let templates_dir = format!(
            "{}/lib/{}/bootstrap/charts/{}/templates",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            EsoRequirementsChart::chart_name(),
        );

        let required_crds = ["crd-clustersecretstore.yaml", "crd-externalsecret.yaml"];

        for crd_file in &required_crds {
            let crd_path = format!("{templates_dir}/{crd_file}");
            let file = std::fs::File::open(&crd_path);
            assert!(file.is_ok(), "CRD template should exist: `{crd_path}`");
        }
    }
}
