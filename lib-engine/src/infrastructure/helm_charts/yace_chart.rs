use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, ChartValuesGenerated, CommonChart, HelmAction, HelmChartError,
    HelmChartNamespaces,
};
use crate::infrastructure::action::metrics_resource_profile::{ResourceProfile, YaceResources};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use kube::Client;
use std::str::FromStr;

pub struct YaceChart {
    action: HelmAction,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    yace_namespace: HelmChartNamespaces,
    cloudwatch_exporter_role_arn: Option<String>,
    aws_region: String,
    cluster_short_id: String,
    query_resources: HelmChartResources,
}

impl YaceChart {
    pub fn new(
        action: HelmAction,
        chart_prefix_path: Option<&str>,
        yace_namespace: HelmChartNamespaces,
        cloudwatch_exporter_role_arn: Option<String>,
        aws_region: String,
        cluster_short_id: String,
        resource_profile: ResourceProfile,
    ) -> Self {
        let yace_resources = YaceResources::get(resource_profile);
        YaceChart {
            action,
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                YaceChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                YaceChart::chart_name(),
            ),
            yace_namespace,
            cloudwatch_exporter_role_arn,
            aws_region,
            cluster_short_id,
            query_resources: HelmChartResources {
                limit_cpu: KubernetesCpuResourceUnit::from_str(&yace_resources.cpu_limit).unwrap(),
                limit_memory: KubernetesMemoryResourceUnit::from_str(&yace_resources.memory_limit).unwrap(),
                request_cpu: KubernetesCpuResourceUnit::from_str(&yace_resources.cpu_request).unwrap(),
                request_memory: KubernetesMemoryResourceUnit::from_str(&yace_resources.memory_request).unwrap(),
            },
        }
    }

    pub fn chart_name() -> String {
        "prometheus-yet-another-cloudwatch-exporter".to_string()
    }

    fn generate_yace_config(&self) -> String {
        format!(
            r#"apiVersion: v1alpha1
sts-region: {}
discovery:
  jobs:
  - type: AWS/RDS
    regions:
      - {}
    searchTags:
      - key: cluster_id
        value: {}
    metrics:
      - name: MaximumUsedTransactionIDs
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: CPUUtilization
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: DatabaseConnections
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: FreeableMemory
        statistics:
          - Average
          - Minimum
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: FreeStorageSpace
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: ReadLatency
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: WriteLatency
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: ReadIOPS
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: WriteIOPS
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: DiskQueueDepth
        statistics:
          - Average
          - Maximum
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: SwapUsage
        statistics:
          - Average
        period: 60
        length: 600
        addCloudwatchTimestamp: true
      - name: ReplicaLag
        statistics:
          - Average
        period: 60
        length: 600

"#,
            self.aws_region, self.aws_region, self.cluster_short_id,
        )
    }
}

impl ToCommonHelmChart for YaceChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let values_files = vec![self.chart_values_path.to_string()];

        Ok(CommonChart {
            chart_info: ChartInfo {
                action: self.action.clone(),
                name: "prometheus-yet-another-cloudwatch-exporter".to_string(),
                path: self.chart_path.to_string(),
                reinstall_chart_if_installed_version_is_below_than: None,
                namespace: self.yace_namespace.clone(),
                values_files,
                values: vec![
                    ChartSetValue {
                        key: "serviceMonitor.interval".to_string(),
                        value: "60s".to_string(),
                    },
                    ChartSetValue {
                        key: "serviceAccount.annotations.eks\\.amazonaws\\.com/role-arn".to_string(),
                        value: self.cloudwatch_exporter_role_arn.clone().unwrap_or("".to_string()),
                    },
                    ChartSetValue {
                        key: "aws.role".to_string(),
                        value: self.cloudwatch_exporter_role_arn.clone().unwrap_or("".to_string()),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: self.query_resources.request_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: self.query_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: self.query_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: self.query_resources.limit_cpu.to_string(),
                    },
                ],
                yaml_files_content: vec![ChartValuesGenerated {
                    filename: "yace_config_generated.yaml".to_string(),
                    yaml_content: format!("config: |-\n  {}", self.generate_yace_config().replace('\n', "\n  ")),
                }],
                ..Default::default()
            },
            chart_installation_checker: None,
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct YaceChartChecker {}

impl YaceChartChecker {
    pub fn new() -> YaceChartChecker {
        YaceChartChecker {}
    }
}

impl Default for YaceChartChecker {
    fn default() -> Self {
        YaceChartChecker::new()
    }
}

impl ChartInstallationChecker for YaceChartChecker {
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

    use crate::infrastructure::action::metrics_resource_profile::ResourceProfile;
    use crate::infrastructure::helm_charts::yace_chart::YaceChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn yace_chart_directory_exists_test() {
        // setup:
        let chart = YaceChart::new(
            HelmAction::Deploy,
            None,
            HelmChartNamespaces::Qovery,
            Some("cloudwatch_exporter_role_arn".to_string()),
            "us-east-1".to_string(),
            "zbe9e2".to_string(),
            ResourceProfile::Normal,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(Kind::Eks),
            ),
            YaceChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn yace_metrics_chart_values_file_exists_test() {
        // setup:
        let chart = YaceChart::new(
            HelmAction::Deploy,
            None,
            HelmChartNamespaces::Qovery,
            Some("cloudwatch_exporter_role_arn".to_string()),
            "us-east-1".to_string(),
            "zbe9e2".to_string(),
            ResourceProfile::Normal,
        );

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
                YaceChart::chart_name(),
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
        let chart = YaceChart::new(
            HelmAction::Deploy,
            None,
            HelmChartNamespaces::Qovery,
            Some("cloudwatch_exporter_role_arn".to_string()),
            "us-east-1".to_string(),
            "zbe9e2".to_string(),
            ResourceProfile::Normal,
        );
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
                    YaceChart::chart_name()
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
