use kube::Client;

use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;

pub struct KarpenterConfigurationChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    cluster_name: String,
    replace_cluster_autoscaler: bool,
    security_group_id: String,
    disk_size_in_gib: i32,
    cluster_id: String,
    cluster_long_id: String,
    organization_id: String,
    organization_long_id: String,
    region: String,
    spot_enabled: bool,
}

impl KarpenterConfigurationChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        cluster_name: String,
        replace_cluster_autoscaler: bool,
        cluster_security_group_id: String,
        disk_size_in_gib: Option<i32>,
        cluster_id: &str,
        cluster_long_id: uuid::Uuid,
        organization_id: &str,
        organization_long_id: uuid::Uuid,
        region: &str,
        spot_enabled: bool,
    ) -> Self {
        let disk_size_in_gib = disk_size_in_gib.expect("disk size should be defined");
        KarpenterConfigurationChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterConfigurationChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterConfigurationChart::chart_name(),
            ),
            cluster_name,
            replace_cluster_autoscaler,
            security_group_id: cluster_security_group_id,
            disk_size_in_gib,
            cluster_id: cluster_id.to_string(),
            cluster_long_id: cluster_long_id.to_string(),
            organization_id: organization_id.to_string(),
            organization_long_id: organization_long_id.to_string(),
            region: region.to_string(),
            spot_enabled,
        }
    }

    pub fn chart_name() -> String {
        "karpenter-configuration".to_string()
    }
}

impl ToCommonHelmChart for KarpenterConfigurationChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KarpenterConfigurationChart::chart_name(),
                action: match self.replace_cluster_autoscaler {
                    true => HelmAction::Deploy,
                    false => HelmAction::Destroy,
                },
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "clusterName".to_string(),
                        value: self.cluster_name.clone(),
                    },
                    ChartSetValue {
                        key: "securityGroupId".to_string(),
                        value: self.security_group_id.clone(),
                    },
                    ChartSetValue {
                        key: "diskSizeInGib".to_string(),
                        value: format!("{}Gi", self.disk_size_in_gib),
                    },
                    ChartSetValue {
                        key: "tags.ClusterId".to_string(),
                        value: self.cluster_id.clone(),
                    },
                    ChartSetValue {
                        key: "tags.ClusterLongId".to_string(),
                        value: self.cluster_long_id.clone(),
                    },
                    ChartSetValue {
                        key: "tags.OrganizationId".to_string(),
                        value: self.organization_id.clone(),
                    },
                    ChartSetValue {
                        key: "tags.OrganizationLongId".to_string(),
                        value: self.organization_long_id.clone(),
                    },
                    ChartSetValue {
                        key: "tags.Region".to_string(),
                        value: self.region.clone(),
                    },
                    ChartSetValue {
                        key: "capacity_type".to_string(),
                        value: match self.spot_enabled {
                            false => "{on-demand}".to_string(),
                            true => "{spot,on-demand}".to_string(),
                        },
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KarpenterChartChecker::new())),
            vertical_pod_autoscaler: None, // enabled in the chart configuration
        })
    }
}

#[derive(Clone)]
pub struct KarpenterChartChecker {}

impl KarpenterChartChecker {
    pub fn new() -> KarpenterChartChecker {
        KarpenterChartChecker {}
    }
}

impl Default for KarpenterChartChecker {
    fn default() -> Self {
        KarpenterChartChecker::new()
    }
}

impl ChartInstallationChecker for KarpenterChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1366): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use uuid::Uuid;

    use crate::cloud_provider::aws::kubernetes::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn karpenter_configuration_chart_directory_exists_test() {
        // setup:
        let chart = KarpenterConfigurationChart::new(
            None,
            "whatever".to_string(),
            true,
            "securitry_group".to_string(),
            Some(50),
            "cluster_id",
            Uuid::new_v4(),
            "organization_id",
            Uuid::new_v4(),
            "region",
            true,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            KarpenterConfigurationChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn karpenter_configuration_chart_values_file_exists_test() {
        // setup:
        let chart = KarpenterConfigurationChart::new(
            None,
            "whatever".to_string(),
            true,
            "securitry_group".to_string(),
            Some(50),
            "cluster_id",
            Uuid::new_v4(),
            "organization_id",
            Uuid::new_v4(),
            "region",
            true,
        );

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
            KarpenterConfigurationChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn karpenter_configuration_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = KarpenterConfigurationChart::new(
            None,
            "whatever".to_string(),
            true,
            "securitry_group".to_string(),
            Some(50),
            "cluster_id",
            Uuid::new_v4(),
            "organization_id",
            Uuid::new_v4(),
            "region",
            true,
        );
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
                KarpenterConfigurationChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
