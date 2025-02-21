use crate::errors::CommandError;
use crate::helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use kube::Client;

pub struct AwsNodeTermHandlerChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    replaced_by_karpenter: bool,
}

impl AwsNodeTermHandlerChart {
    pub fn new(chart_prefix_path: Option<&str>, replaced_by_karpenter: bool) -> AwsNodeTermHandlerChart {
        AwsNodeTermHandlerChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                "aws-node-termination-handler".to_string(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                AwsNodeTermHandlerChart::chart_name(),
            ),
            replaced_by_karpenter,
        }
    }

    pub fn chart_name() -> String {
        "aws-node-term-handler".to_string()
    }
}

impl ToCommonHelmChart for AwsNodeTermHandlerChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: AwsNodeTermHandlerChart::chart_name(),
                action: match self.replaced_by_karpenter {
                    true => HelmAction::Destroy,
                    false => HelmAction::Deploy,
                },
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "nameOverride".to_string(),
                        value: AwsNodeTermHandlerChart::chart_name(),
                    },
                    ChartSetValue {
                        key: "fullnameOverride".to_string(),
                        value: AwsNodeTermHandlerChart::chart_name(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(AwsNodeTermHandlerChecker::new())),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct AwsNodeTermHandlerChecker {}

impl AwsNodeTermHandlerChecker {
    pub fn new() -> AwsNodeTermHandlerChecker {
        AwsNodeTermHandlerChecker {}
    }
}

impl Default for AwsNodeTermHandlerChecker {
    fn default() -> Self {
        AwsNodeTermHandlerChecker::new()
    }
}

impl ChartInstallationChecker for AwsNodeTermHandlerChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1363): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::action::eks::helm_charts::aws_node_term_handler_chart::AwsNodeTermHandlerChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn aws_node_term_handler_chart_directory_exists_test() {
        // setup:
        let chart = AwsNodeTermHandlerChart::new(None, false);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/aws-node-termination-handler/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn aws_node_term_handler_chart_values_file_exists_test() {
        // setup:
        let chart = AwsNodeTermHandlerChart::new(None, false);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            AwsNodeTermHandlerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn aws_node_term_handler_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = AwsNodeTermHandlerChart::new(None, false);
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
                ),
                AwsNodeTermHandlerChart::chart_name()
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
