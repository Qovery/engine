use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use kube::Client;
use semver::Version;

pub struct BeylaChart {
    action: HelmAction,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    beyla_namespace: HelmChartNamespaces,
    additional_char_path: Option<HelmChartValuesFilePath>,
    cluster_name: String,
}

impl BeylaChart {
    pub fn new(
        action: HelmAction,
        chart_prefix_path: Option<&str>,
        beyla_namespace: HelmChartNamespaces,
        cluster_name: &str,
    ) -> Self {
        BeylaChart {
            action,
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                BeylaChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                BeylaChart::chart_name(),
            ),
            beyla_namespace,
            additional_char_path: None,
            cluster_name: cluster_name.to_string(),
        }
    }

    pub fn chart_name() -> String {
        "beyla".to_string()
    }
}

impl ToCommonHelmChart for BeylaChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values_files = vec![self.chart_values_path.to_string()];
        if let Some(additional_char_path) = &self.additional_char_path {
            values_files.push(additional_char_path.to_string());
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                action: self.action.clone(),
                name: "beyla".to_string(),
                path: self.chart_path.to_string(),
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(3, 3, 1)),
                namespace: self.beyla_namespace.clone(),
                values_files,
                values: vec![
                    // query
                    ChartSetValue {
                        key: "env.OTEL_EBPF_KUBE_CLUSTER_NAME".to_string(),
                        value: self.cluster_name.clone(),
                    },
                ],
                yaml_files_content: vec![],
                ..Default::default()
            },
            chart_installation_checker: None,
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct BeylaChartChecker {}

impl BeylaChartChecker {
    pub fn new() -> BeylaChartChecker {
        BeylaChartChecker {}
    }
}

impl Default for BeylaChartChecker {
    fn default() -> Self {
        BeylaChartChecker::new()
    }
}

impl ChartInstallationChecker for BeylaChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1385): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmAction, HelmChartNamespaces};

    use crate::infrastructure::helm_charts::beyla_chart::BeylaChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn beyla_chart_directory_exists_test() {
        // setup:
        let chart = BeylaChart::new(HelmAction::Deploy, None, HelmChartNamespaces::Qovery, "cluster-name");

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            BeylaChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn beyla_metrics_chart_values_file_exists_test() {
        // setup:
        let chart = BeylaChart::new(HelmAction::Deploy, None, HelmChartNamespaces::Qovery, "cluster-name");

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        {
            let provider_kind = Kind::Eks;
            let chart_values_path = format!(
                "{}/lib/{}/bootstrap/chart_values/{}.yaml",
                current_directory
                    .to_str()
                    .expect("Impossible to convert current directory to string"),
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(provider_kind)
                ),
                BeylaChart::chart_name(),
            );

            // execute
            let values_file = std::fs::File::open(&chart_values_path);

            // verify:
            assert!(
                values_file.is_ok(),
                "Chart values {provider_kind} file should exist: `{chart_values_path}`"
            );
        }
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn bayla_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = BeylaChart::new(HelmAction::Deploy, None, HelmChartNamespaces::Qovery, "cluster-name");
        let common_chart = chart.to_common_helm_chart().unwrap();

        {
            let provider_kind = Kind::Eks;
            // execute:
            let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
                common_chart.clone(),
                format!(
                    "/lib/{}/bootstrap/chart_values/{}.yaml",
                    get_helm_path_kubernetes_provider_sub_folder_name(
                        chart.chart_values_path.helm_path(),
                        HelmChartType::CloudProviderSpecific(provider_kind),
                    ),
                    BeylaChart::chart_name()
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
}
