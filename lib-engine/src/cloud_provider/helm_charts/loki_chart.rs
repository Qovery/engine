use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use kube::Client;

pub enum LokiEncryptionType {
    None,
    ServerSideEncryption,
}

#[derive(Default)]
pub struct LokiS3BucketConfiguration {
    pub region: Option<String>,
    pub s3_config: Option<String>,
    pub bucketname: Option<String>,
    pub endpoint: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub insecure: bool,
    pub use_path_style: bool,
}

pub struct LokiChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    encryption_type: LokiEncryptionType,
    chart_namespace: HelmChartNamespaces,
    loki_log_retention_in_weeks: u32,
    loki_s3_bucket_configuration: LokiS3BucketConfiguration,
}

impl LokiChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        encryption_type: LokiEncryptionType,
        chart_namespace: HelmChartNamespaces,
        loki_log_retention_in_weeks: u32,
        loki_s3_bucket_configuration: LokiS3BucketConfiguration,
    ) -> Self {
        LokiChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                LokiChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                LokiChart::chart_name(),
            ),
            encryption_type,
            chart_namespace,
            loki_log_retention_in_weeks,
            loki_s3_bucket_configuration,
        }
    }

    pub fn chart_name() -> String {
        "loki".to_string()
    }
}

impl ToCommonHelmChart for LokiChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: LokiChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.chart_namespace,
                timeout_in_seconds: 900,
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    // AWS
                    ChartSetValue {
                        key: "config.storage_config.aws.s3forcepathstyle".to_string(),
                        value: self.loki_s3_bucket_configuration.use_path_style.to_string(),
                    },
                    ChartSetValue {
                        key: "config.storage_config.aws.s3".to_string(),
                        value: self
                            .loki_s3_bucket_configuration
                            .s3_config
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(), // Qovery setting
                    },
                    ChartSetValue {
                        key: "config.storage_config.aws.region".to_string(),
                        value: self
                            .loki_s3_bucket_configuration
                            .region
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(), // Qovery setting
                    },
                    ChartSetValue {
                        key: "config.storage_config.aws.bucketnames".to_string(),
                        value: self
                            .loki_s3_bucket_configuration
                            .bucketname
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(), // Qovery setting
                    },
                    ChartSetValue {
                        key: "config.storage_config.aws.endpoint".to_string(),
                        value: self
                            .loki_s3_bucket_configuration
                            .endpoint
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(), // Qovery setting
                    },
                    ChartSetValue {
                        key: "config.storage_config.aws.sse_encryption".to_string(),
                        value: match self.encryption_type {
                            LokiEncryptionType::None => "false",
                            LokiEncryptionType::ServerSideEncryption => "true",
                        }
                        .to_string(), // Qovery settings
                    },
                    ChartSetValue {
                        key: "config.storage_config.aws.insecure".to_string(),
                        value: self.loki_s3_bucket_configuration.insecure.to_string(), // Qovery settings
                    },
                    ChartSetValue {
                        key: "config.storage_config.aws.access_key_id".to_string(),
                        value: self
                            .loki_s3_bucket_configuration
                            .access_key_id
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(), // Qovery setting
                    },
                    ChartSetValue {
                        key: "config.storage_config.aws.secret_access_key".to_string(),
                        value: self
                            .loki_s3_bucket_configuration
                            .secret_access_key
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(), // Qovery setting
                    },
                    // Chunk store config
                    ChartSetValue {
                        key: "config.chunk_store_config.max_look_back_period".to_string(),
                        value: format!("{}w", self.loki_log_retention_in_weeks), // Qovery setting (default 12 week)
                    },
                    // Table manager
                    ChartSetValue {
                        key: "config.table_manager.retention_period".to_string(),
                        value: format!("{}w", self.loki_log_retention_in_weeks), // Qovery setting (default 12 week)
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(LokiChartChecker::new())),
        }
    }
}

pub struct LokiChartChecker {}

impl LokiChartChecker {
    pub fn new() -> LokiChartChecker {
        LokiChartChecker {}
    }
}

impl Default for LokiChartChecker {
    fn default() -> Self {
        LokiChartChecker::new()
    }
}

impl ChartInstallationChecker for LokiChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1372): Implement chart install verification
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::HelmChartNamespaces;
    use crate::cloud_provider::helm_charts::loki_chart::{LokiChart, LokiEncryptionType, LokiS3BucketConfiguration};
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn loki_chart_directory_exists_test() {
        // setup:
        let chart = LokiChart::new(
            None,
            LokiEncryptionType::None,
            HelmChartNamespaces::Logging,
            12,
            LokiS3BucketConfiguration::default(),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            LokiChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }

    /// Makes sure chart values file exists.
    #[test]
    fn loki_chart_values_file_exists_test() {
        // setup:
        let chart = LokiChart::new(
            None,
            LokiEncryptionType::None,
            HelmChartNamespaces::Logging,
            12,
            LokiS3BucketConfiguration::default(),
        );

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
            LokiChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn loki_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = LokiChart::new(
            None,
            LokiEncryptionType::None,
            HelmChartNamespaces::Logging,
            12,
            LokiS3BucketConfiguration::default(),
        );
        let common_chart = chart.to_common_helm_chart();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared,
                ),
                LokiChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
