use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use kube::Client;

pub struct AwsNodeTermHandlerChart {
    chart_path: HelmChartPath,
}

impl AwsNodeTermHandlerChart {
    pub fn new(chart_prefix_path: Option<&str>) -> AwsNodeTermHandlerChart {
        AwsNodeTermHandlerChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                "aws-node-termination-handler".to_string(),
            ),
        }
    }

    fn chart_name() -> String {
        "aws-node-term-handler".to_string()
    }
}

impl ToCommonHelmChart for AwsNodeTermHandlerChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: AwsNodeTermHandlerChart::chart_name(),
                path: self.chart_path.to_string(),
                values: vec![
                    ChartSetValue {
                        key: "nameOverride".to_string(),
                        value: AwsNodeTermHandlerChart::chart_name(),
                    },
                    ChartSetValue {
                        key: "fullnameOverride".to_string(),
                        value: AwsNodeTermHandlerChart::chart_name(),
                    },
                    ChartSetValue {
                        key: "enableSpotInterruptionDraining".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "enableScheduledEventDraining".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "deleteLocalData".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "ignoreDaemonSets".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "podTerminationGracePeriod".to_string(),
                        value: "300".to_string(),
                    },
                    ChartSetValue {
                        key: "nodeTerminationGracePeriod".to_string(),
                        value: "120".to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(AwsNodeTermHandlerChecker::new())),
        }
    }
}

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
}
