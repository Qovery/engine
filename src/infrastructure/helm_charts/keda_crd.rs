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

pub struct KedaCrdChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    action: HelmAction,
}

impl KedaCrdChart {
    pub fn new(chart_prefix_path: Option<&str>, action: HelmAction) -> Self {
        KedaCrdChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KedaCrdChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KedaCrdChart::chart_name(),
            ),
            action,
        }
    }

    pub fn chart_name() -> String {
        "keda-crd".to_string()
    }
}

impl ToCommonHelmChart for KedaCrdChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KedaCrdChart::chart_name(),
                action: self.action.clone(),
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                upgrade_retry: Some(ChartInfoUpgradeRetry {
                    nb_retry: 5,
                    delay_in_milli_sec: 3_000,
                }),
                ..Default::default()
            },
            chart_installation_checker: match self.action {
                HelmAction::Deploy => Some(Box::new(KedaCrdChartChecker::new())),
                HelmAction::Destroy => None,
            },
            vertical_pod_autoscaler: None,
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
pub struct KedaCrdChartChecker {}

impl KedaCrdChartChecker {
    pub fn new() -> Self {
        KedaCrdChartChecker {}
    }
}

impl Default for KedaCrdChartChecker {
    fn default() -> Self {
        KedaCrdChartChecker::new()
    }
}

impl ChartInstallationChecker for KedaCrdChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());

        let required_crds = [
            "scaledobjects.keda.sh",
            "scaledjobs.keda.sh",
            "triggerauthentications.keda.sh",
            "clustertriggerauthentications.keda.sh",
            "cloudeventsources.eventing.keda.sh",
            "clustercloudeventsources.eventing.keda.sh",
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
                info!("Successfully verified all 6 KEDA CRDs are installed and established");
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
    fn keda_crd_chart_directory_exists_test() {
        // setup:
        let chart = KedaCrdChart::new(None, HelmAction::Deploy);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            KedaCrdChart::chart_name(),
        );

        // execute
        let chart_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(chart_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn keda_crd_chart_values_file_exists_test() {
        // setup:
        let chart = KedaCrdChart::new(None, HelmAction::Deploy);

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
            KedaCrdChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&values_path);

        // verify:
        assert!(values_file.is_ok(), "Values file should exist: `{values_path}`");
    }

    /// Makes sure all 6 CRD template files exist.
    #[test]
    fn keda_crd_templates_exist_test() {
        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart = KedaCrdChart::new(None, HelmAction::Deploy);
        let templates_dir = format!(
            "{}/lib/{}/bootstrap/charts/{}/templates",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            KedaCrdChart::chart_name(),
        );

        let required_crds = [
            "crd-scaledobjects.yaml",
            "crd-scaledjobs.yaml",
            "crd-triggerauthentications.yaml",
            "crd-clustertriggerauthentications.yaml",
            "crd-cloudeventsources.yaml",
            "crd-clustercloudeventsources.yaml",
        ];

        for crd_file in &required_crds {
            let crd_path = format!("{templates_dir}/{crd_file}");
            let file = std::fs::File::open(&crd_path);
            assert!(file.is_ok(), "CRD template should exist: `{crd_path}`");
        }
    }
}
