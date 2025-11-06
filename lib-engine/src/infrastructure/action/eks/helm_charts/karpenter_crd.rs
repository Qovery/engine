use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInfoUpgradeRetry, ChartInstallationChecker, CommonChart, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::runtime::block_on;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{Api, Client};

pub struct KarpenterCrdChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
}

impl KarpenterCrdChart {
    pub fn new(chart_prefix_path: Option<&str>) -> Self {
        KarpenterCrdChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterCrdChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterCrdChart::chart_name(),
            ),
        }
    }

    pub fn chart_name() -> String {
        "karpenter-crd".to_string()
    }
}

impl ToCommonHelmChart for KarpenterCrdChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KarpenterCrdChart::chart_name(),
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                upgrade_retry: Some(ChartInfoUpgradeRetry {
                    nb_retry: 5,
                    delay_in_milli_sec: 3_000, // 3 seconds between retries
                }),
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KarpenterCrdChartChecker::new())),
            vertical_pod_autoscaler: None, // enabled in the chart configuration
        })
    }
}

#[derive(Clone)]
pub struct KarpenterCrdChartChecker {}

impl KarpenterCrdChartChecker {
    pub fn new() -> KarpenterCrdChartChecker {
        KarpenterCrdChartChecker {}
    }
}

impl Default for KarpenterCrdChartChecker {
    fn default() -> Self {
        KarpenterCrdChartChecker::new()
    }
}

impl ChartInstallationChecker for KarpenterCrdChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());

        let required_crds = [
            "ec2nodeclasses.karpenter.k8s.aws",
            "nodeclaims.karpenter.sh",
            "nodepools.karpenter.sh",
        ];

        for crd_name in &required_crds {
            if let Err(e) = block_on(crds.get(crd_name)) {
                return Err(CommandError::new_from_safe_message(format!(
                    "Karpenter CRD '{}' not found after '{}' chart installation. This indicates a deployment failure. Error: {}",
                    crd_name,
                    KarpenterCrdChart::chart_name(),
                    e
                )));
            }
        }

        info!("Successfully verified all 3 Karpenter CRDs are installed");
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}
