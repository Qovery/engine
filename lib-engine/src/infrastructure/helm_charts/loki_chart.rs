use std::ops::Add;
use std::sync::Arc;

use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::qovery_source_registry::QoverySourceRegistry;
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartTimeout, HelmChartValuesFilePath,
    ToCommonHelmChart, ToHelmChartValue,
};
use crate::infrastructure::models::cloud_provider::Kind;
use crate::io_models::loki::{LokiDeploymentMode, LokiParameters};
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

#[derive(Default)]
pub struct BlobStorageLokiChartConfiguration {
    pub bucketname: Option<String>,
    pub azure_loki_storage_service_account: Option<String>,
    pub azure_loki_msi_client_id: Option<String>,
}

#[derive(Default)]
pub struct LocalLokiChartConfiguration {}

pub enum LokiObjectBucketConfiguration {
    S3(S3LokiChartConfiguration),
    GCS(GCSLokiChartConfiguration),
    BlobStorage(BlobStorageLokiChartConfiguration),
    Local,
}

impl LokiObjectBucketConfiguration {
    fn storage_type_id(&self) -> &'static str {
        match self {
            LokiObjectBucketConfiguration::S3(_) => "s3",
            LokiObjectBucketConfiguration::GCS(_) => "gcs",
            LokiObjectBucketConfiguration::BlobStorage(_) => "azure",
            LokiObjectBucketConfiguration::Local => "filesystem",
        }
    }
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
    customer_helm_chart_vpa_override: Option<CustomerHelmChartsOverride>,
    enable_vpa: bool,
    vpa_min_mcpu: Option<u32>,
    loki_deployment_mode: LokiDeploymentMode,
    write_resources: HelmChartResources,
    read_resources: HelmChartResources,
    backend_resources: HelmChartResources,
    single_binary_resources: HelmChartResources,
    additional_chart_paths: Vec<HelmChartValuesFilePath>,
    chart_timeout: HelmChartTimeout,
    cloud_provider_kind: Kind,
}

impl LokiChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        // encryption_type: LokiEncryptionType,
        chart_namespace: HelmChartNamespaces,
        loki_object_bucket_configuration: LokiObjectBucketConfiguration,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        enable_vpa: bool,
        vpa_min_mcpu: Option<u32>,
        loki_parameters: LokiParameters,
        chart_timeout: HelmChartTimeout,
        karpenter_enabled: bool,
        cloud_provider_kind: Kind,
    ) -> Self {
        let LokiParameters {
            deployment_mode: loki_deployment_mode,
            log_retention_in_week: loki_log_retention_in_weeks,
            write_resources,
            read_resources,
            backend_resources,
            single_binary_resources,
        } = loki_parameters;

        let chart_values_path_directory = match loki_object_bucket_configuration {
            LokiObjectBucketConfiguration::S3(_)
            | LokiObjectBucketConfiguration::GCS(_)
            | LokiObjectBucketConfiguration::BlobStorage(_) => HelmChartDirectoryLocation::CommonFolder,
            LokiObjectBucketConfiguration::Local => HelmChartDirectoryLocation::CloudProviderFolder,
        };

        let mut additional_chart_paths = vec![];

        // Local storage already ships single-binary via its provider base values;
        // only object-storage clusters need the SingleBinary overlay to flip the default.
        if matches!(loki_deployment_mode, LokiDeploymentMode::SingleBinary)
            && !matches!(loki_object_bucket_configuration, LokiObjectBucketConfiguration::Local)
        {
            additional_chart_paths.push(HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                "loki_single_binary".to_string(),
            ));
        }

        if karpenter_enabled {
            additional_chart_paths.push(HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                "loki_with_karpenter".to_string(),
            ));
        }

        LokiChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                LokiChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                chart_values_path_directory,
                LokiChart::chart_name(),
            ),
            // encryption_type,
            chart_namespace,
            loki_log_retention_in_weeks,
            loki_object_bucket_configuration,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            customer_helm_chart_vpa_override: customer_helm_chart_fn(Self::chart_name().add(".vpa")),
            enable_vpa,
            vpa_min_mcpu,
            loki_deployment_mode,
            write_resources,
            read_resources,
            backend_resources,
            single_binary_resources,
            additional_chart_paths,
            chart_timeout,
            cloud_provider_kind,
        }
    }

    pub fn chart_name() -> String {
        "loki".to_string()
    }

    /// Local storage forces single-binary regardless of the requested deployment mode.
    fn is_single_binary_effective(&self) -> bool {
        matches!(self.loki_object_bucket_configuration, LokiObjectBucketConfiguration::Local)
            || matches!(self.loki_deployment_mode, LokiDeploymentMode::SingleBinary)
    }
}

impl ToCommonHelmChart for LokiChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        // getting both S3 and GCS configuration, default will be used if not set
        let default_gcs_configuration = GCSLokiChartConfiguration::default();
        let default_s3_configuration = S3LokiChartConfiguration::default();
        let default_blob_storage_configuration = BlobStorageLokiChartConfiguration::default();
        let (gcs_configuration, s3_configuration, blob_storage_configuration) =
            match &self.loki_object_bucket_configuration {
                LokiObjectBucketConfiguration::S3(config) => {
                    (&default_gcs_configuration, config, &default_blob_storage_configuration)
                }
                LokiObjectBucketConfiguration::GCS(config) => {
                    (config, &default_s3_configuration, &default_blob_storage_configuration)
                }
                LokiObjectBucketConfiguration::BlobStorage(config) => {
                    (&default_gcs_configuration, &default_s3_configuration, config)
                }
                LokiObjectBucketConfiguration::Local => (
                    &default_gcs_configuration,
                    &default_s3_configuration,
                    &default_blob_storage_configuration,
                ),
            };

        let bucket_name = match &self.loki_object_bucket_configuration {
            LokiObjectBucketConfiguration::S3(c) => c.bucketname.as_deref().unwrap_or("").to_string(),
            LokiObjectBucketConfiguration::GCS(c) => c.bucketname.as_deref().unwrap_or("").to_string(),
            LokiObjectBucketConfiguration::BlobStorage(c) => c.bucketname.as_deref().unwrap_or("").to_string(),
            LokiObjectBucketConfiguration::Local => "".to_string(),
        };
        let object_store_value = self.loki_object_bucket_configuration.storage_type_id();

        let mut values_files = vec![self.chart_values_path.to_string()];
        for path in &self.additional_chart_paths {
            values_files.push(path.to_string());
        }

        let mut common_chart = CommonChart {
            chart_info: ChartInfo {
                name: LokiChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.chart_namespace.clone(),
                timeout_in_seconds: match self.chart_timeout {
                    HelmChartTimeout::ChartDefault => 900,
                    HelmChartTimeout::Custom(t) => t.whole_seconds(),
                },
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(6, 0, 0)),
                values_files,
                values: {
                    let source_registry = QoverySourceRegistry::from(&self.cloud_provider_kind);
                    vec![
                        ChartSetValue {
                            key: "loki.image.registry".to_string(),
                            value: source_registry.host(),
                        },
                        ChartSetValue {
                            key: "loki.image.repository".to_string(),
                            value: source_registry.image_path("pub-mirror-loki"),
                        },
                        // Disable sidecar for rules - not used by Qovery
                        ChartSetValue {
                            key: "sidecar.rules.enabled".to_string(),
                            value: "false".to_string(),
                        },
                        ChartSetValue {
                            key: "loki.compactor.retention_enabled".to_string(),
                            value: "true".to_string(),
                        },
                        ChartSetValue {
                            key: "loki.compactor.delete_request_store".to_string(),
                            value: object_store_value.to_string(),
                        },
                        // Logs retention period, table manager will be removed in the future (only used for boltdb-shipper)
                        ChartSetValue {
                            key: "loki.limits_config.retention_period".to_string(),
                            value: format!("{}w", self.loki_log_retention_in_weeks), // (default 12 week)
                        },
                        ChartSetValue {
                            key: "loki.storage.type".to_string(),
                            value: object_store_value.to_string(),
                        },
                        // Schema configuration object_store settings
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
                            value: s3_configuration.s3_config.as_deref().unwrap_or("").to_string(),
                        },
                        ChartSetValue {
                            key: "loki.storage.s3.region".to_string(),
                            value: s3_configuration.region.as_deref().unwrap_or("").to_string(), // Qovery setting
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
                                .as_deref()
                                .unwrap_or("")
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
                                .as_deref()
                                .unwrap_or("")
                                .to_string(),
                        },
                        // Azure blob storage configuration
                        ChartSetValue {
                            key: "loki.storage.azure.account_name".to_string(),
                            value: blob_storage_configuration
                                .azure_loki_storage_service_account
                                .as_deref()
                                .unwrap_or("")
                                .to_string(),
                        },
                        ChartSetValue {
                            key: "loki.storage.azure.container_name".to_string(),
                            value: bucket_name.to_string(),
                        },
                        ChartSetValue {
                            key: "loki.storage_config.azure.container_name".to_string(),
                            value: bucket_name.to_string(),
                        },
                        ChartSetValue {
                            key: "loki.storage_config.azure.account_name".to_string(),
                            value: blob_storage_configuration
                                .azure_loki_storage_service_account
                                .as_deref()
                                .unwrap_or("")
                                .to_string(),
                        },
                    ]
                },
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
                            if self.is_single_binary_effective() {
                                "loki".to_string()
                            } else {
                                "loki-write".to_string()
                            },
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(self.vpa_min_mcpu.unwrap_or(300))),
                            Some(KubernetesCpuResourceUnit::MilliCpu(2000)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(8)),
                        ),
                        customer_helm_chart_override: self.customer_helm_chart_vpa_override.clone(),
                    }],
                )),
                false => None,
            },
            pre_execute_action: None,
        };

        // Add schema configuration object_store settings
        common_chart.chart_info.values.extend([
            ChartSetValue {
                key: "loki.schemaConfig.configs[0].object_store".to_string(),
                value: object_store_value.to_string(),
            },
            ChartSetValue {
                key: "loki.schemaConfig.configs[1].object_store".to_string(),
                value: object_store_value.to_string(),
            },
            ChartSetValue {
                key: "loki.schemaConfig.configs[2].object_store".to_string(),
                value: object_store_value.to_string(),
            },
        ]);

        // Specific Azure blob storage configuration
        if let LokiObjectBucketConfiguration::BlobStorage(_azure_blob_storage_config) =
            &self.loki_object_bucket_configuration
        {
            // Add this label to the Loki pods to enable workload identity
            common_chart.chart_info.values_string.push(ChartSetValue {
                key: r"loki.podLabels.azure\.workload\.identity/use".to_string(),
                value: "true".to_string(),
            });
            common_chart.chart_info.values.push(ChartSetValue {
                key: r"serviceAccount.name".to_string(),
                value: "qovery-storage".to_string(),
            });
            common_chart.chart_info.values_string.push(ChartSetValue {
                key: r"serviceAccount.labels.azure\.workload\.identity/use".to_string(),
                value: "true".to_string(),
            });
            common_chart.chart_info.values.push(ChartSetValue {
                key: r"serviceAccount.annotations.azure\.workload\.identity/client-id".to_string(),
                value: blob_storage_configuration
                    .azure_loki_msi_client_id
                    .as_deref()
                    .unwrap_or("")
                    .to_string(),
            });
        }

        let inject_resources = |component: &str, r: &HelmChartResources| -> [ChartSetValue; 4] {
            [
                ChartSetValue {
                    key: format!("{component}.resources.requests.cpu"),
                    value: r.request_cpu.to_helm_chart_value(),
                },
                ChartSetValue {
                    key: format!("{component}.resources.limits.cpu"),
                    value: r.limit_cpu.to_helm_chart_value(),
                },
                ChartSetValue {
                    key: format!("{component}.resources.requests.memory"),
                    value: r.request_memory.to_helm_chart_value(),
                },
                ChartSetValue {
                    key: format!("{component}.resources.limits.memory"),
                    value: r.limit_memory.to_helm_chart_value(),
                },
            ]
        };

        if self.is_single_binary_effective() {
            common_chart
                .chart_info
                .values
                .extend(inject_resources("singleBinary", &self.single_binary_resources));
        } else {
            common_chart
                .chart_info
                .values
                .extend(inject_resources("write", &self.write_resources));
            common_chart
                .chart_info
                .values
                .extend(inject_resources("read", &self.read_resources));
            common_chart
                .chart_info
                .values
                .extend(inject_resources("backend", &self.backend_resources));
        }

        Ok(common_chart)
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
        HelmChartTimeout, HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::cloud_provider::Kind;
    use crate::io_models::loki::{LokiDeploymentMode, LokiParameters};
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
            HelmChartNamespaces::Logging,
            LokiObjectBucketConfiguration::S3(S3LokiChartConfiguration::default()),
            get_loki_chart_override(),
            false,
            None,
            LokiParameters {
                deployment_mode: LokiDeploymentMode::SimpleScalable,
                log_retention_in_week: 12,
                ..Default::default()
            },
            HelmChartTimeout::ChartDefault,
            false,
            Kind::Aws,
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
            HelmChartNamespaces::Logging,
            LokiObjectBucketConfiguration::S3(S3LokiChartConfiguration::default()),
            get_loki_chart_override(),
            false,
            None,
            LokiParameters {
                deployment_mode: LokiDeploymentMode::SimpleScalable,
                log_retention_in_week: 12,
                ..Default::default()
            },
            HelmChartTimeout::ChartDefault,
            true,
            Kind::Aws,
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
            HelmChartNamespaces::Logging,
            LokiObjectBucketConfiguration::S3(S3LokiChartConfiguration::default()),
            get_loki_chart_override(),
            false,
            None,
            LokiParameters {
                deployment_mode: LokiDeploymentMode::SimpleScalable,
                log_retention_in_week: 12,
                ..Default::default()
            },
            HelmChartTimeout::ChartDefault,
            false,
            Kind::Aws,
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
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }
}
