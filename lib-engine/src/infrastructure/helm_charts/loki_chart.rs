use std::sync::Arc;

use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartResourcesConstraintType, HelmChartTimeout,
    HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::models::{CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

use kube::Client;
use semver::Version;

// TODO: reintroduce encryption type when chart is fixed. Can't be set ATM: https://github.com/grafana/loki/issues/9018

pub enum LokiEncryptionType {
    None,
    ServerSideEncryption,
}

#[derive(Default)]
pub struct S3LokiChartConfiguration {
    pub region: Option<String>,
    pub s3_config: Option<String>,
    pub bucketname: Option<String>,
    pub insecure: bool,
    pub use_path_style: bool,
    pub aws_iam_loki_role_arn: Option<String>,
}

#[derive(Default)]
pub struct GCSLokiChartConfiguration {
    pub bucketname: Option<String>,
    pub gcp_service_account: Option<String>,
}

pub enum LokiObjectBucketConfiguration {
    S3(S3LokiChartConfiguration),
    GCS(GCSLokiChartConfiguration),
}

pub struct LokiChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    // encryption_type: LokiEncryptionType,
    chart_namespace: HelmChartNamespaces,
    loki_log_retention_in_weeks: u32,
    loki_object_bucket_configuration: LokiObjectBucketConfiguration,
    customer_helm_chart_override: Option<CustomerHelmChartsOverride>,
    enable_vpa: bool,
    vpa_min_mcpu: Option<u32>,
    chart_resources: HelmChartResources,
    additional_char_path: Option<HelmChartValuesFilePath>,
    chart_timeout: HelmChartTimeout,
}

impl LokiChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        // encryption_type: LokiEncryptionType,
        chart_namespace: HelmChartNamespaces,
        loki_log_retention_in_weeks: u32,
        loki_object_bucket_configuration: LokiObjectBucketConfiguration,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        enable_vpa: bool,
        vpa_min_mcpu: Option<u32>,
        chart_resources: HelmChartResourcesConstraintType,
        chart_timeout: HelmChartTimeout,
        karpenter_enabled: bool,
    ) -> Self {
        LokiChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
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
            // encryption_type,
            chart_namespace,
            loki_log_retention_in_weeks,
            loki_object_bucket_configuration,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            enable_vpa,
            vpa_min_mcpu,
            chart_resources: match chart_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(300),
                    request_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(2),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            additional_char_path: match karpenter_enabled {
                true => Some(HelmChartValuesFilePath::new(
                    chart_prefix_path,
                    HelmChartDirectoryLocation::CommonFolder,
                    "loki_with_karpenter".to_string(),
                )),
                false => None,
            },
            chart_timeout,
        }
    }

    pub fn chart_name() -> String {
        "loki".to_string()
    }
}

impl ToCommonHelmChart for LokiChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        // getting both S3 and GCS configuration, default will be used if not set
        let default_gcs_configuration = GCSLokiChartConfiguration::default();
        let default_s3_configuration = S3LokiChartConfiguration::default();
        let (gcs_configuration, s3_configuration) = match &self.loki_object_bucket_configuration {
            LokiObjectBucketConfiguration::S3(config) => (&default_gcs_configuration, config),
            LokiObjectBucketConfiguration::GCS(config) => (config, &default_s3_configuration),
        };

        let bucket_name = match &self.loki_object_bucket_configuration {
            LokiObjectBucketConfiguration::S3(c) => c.bucketname.as_ref().unwrap_or(&"".to_string()).to_string(),
            LokiObjectBucketConfiguration::GCS(c) => c.bucketname.as_ref().unwrap_or(&"".to_string()).to_string(),
        };

        let mut values_files = vec![self.chart_values_path.to_string()];
        if let Some(additional_char_path) = &self.additional_char_path {
            values_files.push(additional_char_path.to_string());
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: LokiChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.chart_namespace,
                timeout_in_seconds: match self.chart_timeout {
                    HelmChartTimeout::ChartDefault => 900,
                    HelmChartTimeout::Custom(t) => t.whole_seconds(),
                },
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(5, 0, 0)),
                values_files,
                values: vec![
                    ChartSetValue {
                        key: "kubectlImage.registry".to_string(),
                        value: "public.ecr.aws".to_string(),
                    },
                    ChartSetValue {
                        key: "kubectlImage.repository".to_string(),
                        value: "r3m4q3r9/pub-mirror-kubectl".to_string(),
                    },
                    ChartSetValue {
                        key: "loki.image.registry".to_string(),
                        value: "public.ecr.aws".to_string(),
                    },
                    ChartSetValue {
                        key: "loki.image.repository".to_string(),
                        value: "r3m4q3r9/pub-mirror-loki".to_string(),
                    },
                    ChartSetValue {
                        key: "loki.compactor.retention_enabled".to_string(),
                        value: "true".to_string(),
                    },
                    // Table manager
                    // todo(pmavro): ensure there is no better options to handle retention:
                    // https://github.com/grafana/loki/issues/9207, chart is manually overriden
                    ChartSetValue {
                        key: "tableManager.retention_deletes_enabled".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "tableManager.retention_period".to_string(),
                        value: format!("{}w", self.loki_log_retention_in_weeks), // (default 12 week)
                    },
                    // resources limits
                    ChartSetValue {
                        key: "singleBinary.resources.limits.cpu".to_string(),
                        value: self.chart_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "singleBinary.resources.limits.memory".to_string(),
                        value: self.chart_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "singleBinary.resources.requests.cpu".to_string(),
                        value: self.chart_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "singleBinary.resources.requests.memory".to_string(),
                        value: self.chart_resources.request_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "loki.storage.type".to_string(),
                        value: match &self.loki_object_bucket_configuration {
                            LokiObjectBucketConfiguration::S3(_) => "s3",
                            LokiObjectBucketConfiguration::GCS(_) => "gcs",
                        }
                        .to_string(),
                    },
                    ChartSetValue {
                        key: "loki.storage_config.boltdb_shipper.shared_store".to_string(),
                        value: match &self.loki_object_bucket_configuration {
                            LokiObjectBucketConfiguration::S3(_) => "s3",
                            LokiObjectBucketConfiguration::GCS(_) => "gcs",
                        }
                        .to_string(),
                    },
                    ChartSetValue {
                        key: "loki.compactor.shared_store".to_string(),
                        value: match &self.loki_object_bucket_configuration {
                            LokiObjectBucketConfiguration::S3(_) => "aws",
                            LokiObjectBucketConfiguration::GCS(_) => "gcs",
                        }
                        .to_string(),
                    },
                    ChartSetValue {
                        key: "loki.storage.bucketNames.chunks".to_string(),
                        value: bucket_name.to_string(),
                    },
                    ChartSetValue {
                        key: "loki.storage.bucketNames.ruler".to_string(),
                        value: bucket_name.to_string(),
                    },
                    ChartSetValue {
                        key: "loki.storage.bucketNames.admin".to_string(),
                        value: bucket_name.to_string(),
                    },
                    // S3 configuration
                    ChartSetValue {
                        key: "loki.storage.s3.s3ForcePathStyle".to_string(),
                        value: s3_configuration.use_path_style.to_string(),
                    },
                    ChartSetValue {
                        key: "loki.storage.s3.s3".to_string(),
                        value: s3_configuration
                            .s3_config
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(),
                    },
                    ChartSetValue {
                        key: "loki.storage.s3.region".to_string(),
                        value: s3_configuration.region.as_ref().unwrap_or(&"".to_string()).to_string(), // Qovery setting
                    },
                    // Can't be set ATM: https://github.com/grafana/loki/issues/9018
                    // ChartSetValue {
                    //     key: "loki.storage.s3.sse-encryption".to_string(),
                    //     value: match self.encryption_type {
                    //         LokiEncryptionType::None => "false",
                    //         LokiEncryptionType::ServerSideEncryption => "true",
                    //     }
                    //     .to_string(),
                    // },
                    ChartSetValue {
                        key: "loki.storage.s3.insecure".to_string(),
                        value: s3_configuration.insecure.to_string(),
                    },
                    ChartSetValue {
                        // we use string templating (r"...") to escape dot in annotation's key
                        key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: s3_configuration
                            .aws_iam_loki_role_arn
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(),
                    },
                    // GCS configuration
                    ChartSetValue {
                        key: "loki.storage_config.gcs.bucket_name".to_string(),
                        value: bucket_name.to_string(),
                    },
                    ChartSetValue {
                        // we use string templating (r"...") to escape dot in annotation's key
                        key: r"serviceAccount.annotations.iam\.gke\.io/gcp-service-account".to_string(),
                        value: gcs_configuration
                            .gcp_service_account
                            .as_ref()
                            .unwrap_or(&"".to_string())
                            .to_string(),
                    },
                ],
                yaml_files_content: match self.customer_helm_chart_override.clone() {
                    Some(x) => vec![x.to_chart_values_generated()],
                    None => vec![],
                },
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(LokiChartChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::StatefulSet,
                            "loki".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(self.vpa_min_mcpu.unwrap_or(200))),
                            Some(KubernetesCpuResourceUnit::MilliCpu(1000)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(512)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(4)),
                        ),
                    }],
                )),
                false => None,
            },
        })
    }
}

#[derive(Clone)]
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

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::loki_chart::{
        LokiChart, LokiObjectBucketConfiguration, S3LokiChartConfiguration,
    };
    use crate::infrastructure::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartResourcesConstraintType, HelmChartTimeout, HelmChartType, ToCommonHelmChart,
    };
    use crate::io_models::models::CustomerHelmChartsOverride;
    use std::env;
    use std::sync::Arc;

    fn get_loki_chart_override() -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
        Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> {
            Some(CustomerHelmChartsOverride {
                chart_name: LokiChart::chart_name(),
                chart_values: "".to_string(),
            })
        })
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn loki_chart_directory_exists_test() {
        // setup:
        let chart = LokiChart::new(
            None,
            // LokiEncryptionType::None,
            HelmChartNamespaces::Logging,
            12,
            LokiObjectBucketConfiguration::S3(S3LokiChartConfiguration::default()),
            get_loki_chart_override(),
            false,
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartTimeout::ChartDefault,
            false,
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
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn loki_chart_values_file_exists_test() {
        // setup:
        let chart = LokiChart::new(
            None,
            // LokiEncryptionType::None,
            HelmChartNamespaces::Logging,
            12,
            LokiObjectBucketConfiguration::S3(S3LokiChartConfiguration::default()),
            get_loki_chart_override(),
            false,
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartTimeout::ChartDefault,
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
                HelmChartType::Shared,
            ),
            LokiChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn loki_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = LokiChart::new(
            None,
            // LokiEncryptionType::None,
            HelmChartNamespaces::Logging,
            12,
            LokiObjectBucketConfiguration::S3(S3LokiChartConfiguration::default()),
            get_loki_chart_override(),
            false,
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartTimeout::ChartDefault,
            false,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

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
