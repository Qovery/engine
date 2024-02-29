use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use kube::Client;

pub struct KarpenterConfigurationChart {
    chart_path: HelmChartPath,
    cluster_name: String,
    replace_cluster_autoscaler: bool,
    security_group_id: String,
    disk_size_in_gib: i32,
    cluster_id: String,
    cluster_long_id: String,
    organization_id: String,
    organization_long_id: String,
    region: String,
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
    ) -> Self {
        let disk_size_in_gib = disk_size_in_gib.expect("disk size should be defined");
        KarpenterConfigurationChart {
            chart_path: HelmChartPath::new(
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
                        key: "tags.clusterId".to_string(),
                        value: self.cluster_id.clone(),
                    },
                    ChartSetValue {
                        key: "tags.clusterLongId".to_string(),
                        value: self.cluster_long_id.clone(),
                    },
                    ChartSetValue {
                        key: "tags.organizationId".to_string(),
                        value: self.organization_id.clone(),
                    },
                    ChartSetValue {
                        key: "tags.organizationLongId".to_string(),
                        value: self.organization_long_id.clone(),
                    },
                    ChartSetValue {
                        key: "tags.region".to_string(),
                        value: self.region.clone(),
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
