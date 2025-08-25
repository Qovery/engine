use derive_more::Display;

use super::{
    HelmChartAutoscaling, HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartValuesFilePath,
    ToCommonHelmChart, kube_prometheus_stack_chart::StorageClassName,
};
use crate::environment::models::ToCloudProviderFormat;
use crate::helm::HelmAction;
use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::PrometheusConfiguration;
use crate::{
    helm::{ChartInfo, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces},
    io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit},
};

pub struct ThanosChart {
    action: HelmAction,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    prometheus_namespace: HelmChartNamespaces,
    thanos_namespace: HelmChartNamespaces,
    retention: MetricsRetention,
    prometheus_configuration: PrometheusConfiguration,
    storage_class_name: StorageClassName,
    query_resources: HelmChartResources,
    query_autoscaling: HelmChartAutoscaling,
    query_frontend_resources: HelmChartResources,
    query_frontend_autoscaling: HelmChartAutoscaling,
    compactor_resources: HelmChartResources,
    store_gateway_resources: HelmChartResources,
    store_gateway_autoscaling: HelmChartAutoscaling,
    additional_char_path: Option<HelmChartValuesFilePath>,
}

#[derive(Display)]
#[display("{}d", _0)]
pub struct RetentionDays(i32);

pub struct MetricsRetention {
    // no downsampling, raw data (all data) retention (number in days)
    pub retention_resolution_raw_in_days: RetentionDays,
    // downsampled data retention 5m (number in days)
    pub retention_resolution_5m_in_days: RetentionDays,
    // downsampled data retention 1h (number in days)
    pub retention_resolution_1h_in_days: RetentionDays,
}

impl ThanosChart {
    pub fn new(
        action: HelmAction,
        chart_prefix_path: Option<&str>,
        prometheus_namespace: HelmChartNamespaces,
        retention: Option<MetricsRetention>,
        prometheus_configuration: PrometheusConfiguration,
        storage_class_name: StorageClassName,
        query_resources: Option<HelmChartResources>,
        query_frontend_resources: Option<HelmChartResources>,
        compactor_resources: Option<HelmChartResources>,
        store_gateway_resources: Option<HelmChartResources>,
        karpenter_enabled: bool,
        enable_redundancy: bool,
    ) -> Self {
        Self {
            action,
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                ThanosChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                ThanosChart::chart_name(),
            ),
            prometheus_configuration,
            storage_class_name,
            prometheus_namespace: prometheus_namespace.clone(),
            thanos_namespace: prometheus_namespace,
            retention: match retention {
                Some(retention) => retention,
                None => MetricsRetention {
                    retention_resolution_raw_in_days: RetentionDays(15),
                    retention_resolution_5m_in_days: RetentionDays(30),
                    retention_resolution_1h_in_days: RetentionDays(30),
                },
            },
            query_resources: match query_resources {
                Some(resources) => resources,
                None => HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(768),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(768),
                },
            },
            query_autoscaling: HelmChartAutoscaling {
                min_replicas: if enable_redundancy { 2 } else { 1 },
                max_replicas: 5,
                target_cpu_utilization_percentage: 60,
            },
            query_frontend_resources: match query_frontend_resources {
                Some(resources) => resources,
                None => HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(500),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(256),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(500),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(256),
                },
            },
            query_frontend_autoscaling: HelmChartAutoscaling {
                min_replicas: 1,
                max_replicas: 5,
                target_cpu_utilization_percentage: 60,
            },
            compactor_resources: match compactor_resources {
                Some(resources) => resources,
                None => HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(2000),
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(4),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(2000),
                    request_memory: KubernetesMemoryResourceUnit::GibiByte(4),
                },
            },
            store_gateway_resources: match store_gateway_resources {
                Some(resources) => resources,
                None => HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(500),
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(500),
                    request_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                },
            },
            store_gateway_autoscaling: HelmChartAutoscaling {
                min_replicas: if enable_redundancy { 2 } else { 1 },
                max_replicas: 5,
                target_cpu_utilization_percentage: 60,
            },
            additional_char_path: match karpenter_enabled {
                true => Some(HelmChartValuesFilePath::new(
                    chart_prefix_path,
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "thanos-with-karpenter".to_string(),
                )),
                false => None,
            },
        }
    }

    pub fn chart_name() -> String {
        "thanos".to_string()
    }
}

impl ToCommonHelmChart for ThanosChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values_files = vec![self.chart_values_path.to_string()];
        if let Some(additional_char_path) = &self.additional_char_path {
            values_files.push(additional_char_path.to_string());
        }

        let mut chart_info = ChartInfo {
            action: self.action.clone(),
            name: ThanosChart::chart_name(),
            path: self.chart_path.to_string(),
            namespace: self.thanos_namespace.clone(),
            values_files,
            values: vec![
                // query
                ChartSetValue {
                    key: "query.replicaCount".to_string(),
                    value: self.query_autoscaling.min_replicas.to_string(),
                },
                ChartSetValue {
                    key: "query.resources.limits.cpu".to_string(),
                    value: self.query_resources.limit_cpu.to_string(),
                },
                ChartSetValue {
                    key: "query.resources.limits.memory".to_string(),
                    value: self.query_resources.limit_memory.to_string(),
                },
                ChartSetValue {
                    key: "query.resources.requests.cpu".to_string(),
                    value: self.query_resources.request_cpu.to_string(),
                },
                ChartSetValue {
                    key: "query.resources.requests.memory".to_string(),
                    value: self.query_resources.request_memory.to_string(),
                },
                ChartSetValue {
                    key: "query.autoscaling.minReplicas".to_string(),
                    value: self.query_autoscaling.min_replicas.to_string(),
                },
                ChartSetValue {
                    key: "query.autoscaling.maxReplicas".to_string(),
                    value: self.query_autoscaling.max_replicas.to_string(),
                },
                ChartSetValue {
                    key: "query.autoscaling.targetCPU".to_string(),
                    value: self.query_autoscaling.target_cpu_utilization_percentage.to_string(),
                },
                ChartSetValue {
                    key: "query.dnsDiscovery.sidecarsNamespace".to_string(),
                    value: self.prometheus_namespace.to_string(),
                },
                ChartSetValue {
                    key: "query.pdb.create".to_string(),
                    value: if self.query_autoscaling.min_replicas > 1 {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    },
                },
                ChartSetValue {
                    key: "query.pdb.maxUnavailable".to_string(),
                    value: "20%".to_string(),
                },
                // query Frontend
                ChartSetValue {
                    key: "queryFrontend.replicaCount".to_string(),
                    value: "0".to_string(), // disable by default
                },
                ChartSetValue {
                    key: "queryFrontend.resources.limits.cpu".to_string(),
                    value: self.query_frontend_resources.limit_cpu.to_string(),
                },
                ChartSetValue {
                    key: "queryFrontend.resources.limits.memory".to_string(),
                    value: self.query_frontend_resources.limit_memory.to_string(),
                },
                ChartSetValue {
                    key: "queryFrontend.resources.requests.cpu".to_string(),
                    value: self.query_frontend_resources.request_cpu.to_string(),
                },
                ChartSetValue {
                    key: "queryFrontend.resources.requests.memory".to_string(),
                    value: self.query_frontend_resources.request_memory.to_string(),
                },
                ChartSetValue {
                    key: "queryFrontend.autoscaling.minReplicas".to_string(),
                    value: self.query_frontend_autoscaling.min_replicas.to_string(),
                },
                ChartSetValue {
                    key: "queryFrontend.autoscaling.maxReplicas".to_string(),
                    value: self.query_frontend_autoscaling.max_replicas.to_string(),
                },
                ChartSetValue {
                    key: "queryFrontend.autoscaling.targetCPU".to_string(),
                    value: self
                        .query_frontend_autoscaling
                        .target_cpu_utilization_percentage
                        .to_string(),
                },
                // compactor
                ChartSetValue {
                    key: "compactor.concurrency".to_string(),
                    // goroutine per CPU core. Set it to 2x the number of CPU cores, should be fine due to the nature of this job
                    // value: (u32::from(self.compactor_resources.request_cpu.clone()) * 2 / 1000).to_string(),

                    // The memory seems to be the bottleneck, so test with 1 for the moment
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "compactor.retentionResolutionRaw".to_string(),
                    value: self.retention.retention_resolution_raw_in_days.to_string(),
                },
                ChartSetValue {
                    key: "compactor.retentionResolution5m".to_string(),
                    value: self.retention.retention_resolution_5m_in_days.to_string(),
                },
                ChartSetValue {
                    key: "compactor.retentionResolution1h".to_string(),
                    value: self.retention.retention_resolution_1h_in_days.to_string(),
                },
                ChartSetValue {
                    key: "compactor.resources.limits.cpu".to_string(),
                    value: self.compactor_resources.limit_cpu.to_string(),
                },
                ChartSetValue {
                    key: "compactor.resources.limits.memory".to_string(),
                    value: self.compactor_resources.limit_memory.to_string(),
                },
                ChartSetValue {
                    key: "compactor.resources.requests.cpu".to_string(),
                    value: self.compactor_resources.request_cpu.to_string(),
                },
                ChartSetValue {
                    key: "compactor.resources.requests.memory".to_string(),
                    value: self.compactor_resources.request_memory.to_string(),
                },
                // store gateway
                ChartSetValue {
                    key: "storegateway.replicaCount".to_string(),
                    value: self.store_gateway_autoscaling.min_replicas.to_string(),
                },
                ChartSetValue {
                    key: "storegateway.resources.limits.cpu".to_string(),
                    value: self.store_gateway_resources.limit_cpu.to_string(),
                },
                ChartSetValue {
                    key: "storegateway.resources.limits.memory".to_string(),
                    value: self.store_gateway_resources.limit_memory.to_string(),
                },
                ChartSetValue {
                    key: "storegateway.resources.requests.cpu".to_string(),
                    value: self.store_gateway_resources.request_cpu.to_string(),
                },
                ChartSetValue {
                    key: "storegateway.resources.requests.memory".to_string(),
                    value: self.store_gateway_resources.request_memory.to_string(),
                },
                ChartSetValue {
                    key: "storegateway.autoscaling.minReplicas".to_string(),
                    value: self.store_gateway_autoscaling.min_replicas.to_string(),
                },
                ChartSetValue {
                    key: "storegateway.autoscaling.maxReplicas".to_string(),
                    value: self.store_gateway_autoscaling.max_replicas.to_string(),
                },
                ChartSetValue {
                    key: "storegateway.autoscaling.targetCPU".to_string(),
                    value: self
                        .store_gateway_autoscaling
                        .target_cpu_utilization_percentage
                        .to_string(),
                },
                ChartSetValue {
                    key: "storegateway.pdb.create".to_string(),
                    value: if self.store_gateway_autoscaling.min_replicas > 1 {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    },
                },
                ChartSetValue {
                    key: "storegateway.pdb.maxUnavailable".to_string(),
                    value: "20%".to_string(),
                },
                ChartSetValue {
                    key: "storegateway.persistence.storageClass".to_string(),
                    value: self.storage_class_name.to_string(),
                },
            ],
            ..Default::default()
        };
        match &self.prometheus_configuration {
            PrometheusConfiguration::AwsS3 {
                region,
                bucket_name,
                endpoint,
                aws_iam_prometheus_role_arn,
            } => {
                chart_info.values.push(ChartSetValue {
                    key: r"storegateway.serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                    value: aws_iam_prometheus_role_arn.clone(),
                });
                chart_info.values.push(ChartSetValue {
                    key: r"compactor.serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                    value: aws_iam_prometheus_role_arn.clone(),
                });
                chart_info.values.push(ChartSetValue {
                    key: r"bucketweb.serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                    value: aws_iam_prometheus_role_arn.clone(),
                });

                // INFO (ENG-1986) Pass the whole objstoreConfig as a string, as it is expected to be the content of a generated secret
                let region_str = region.to_cloud_provider_format();
                chart_info.values_string.push(ChartSetValue {
                    key: "objstoreConfig".to_string(),
                    value: format!(
                        "type: S3\nconfig:\n  aws_sdk_auth: true\n  bucket: {bucket_name}\n  endpoint: {endpoint}\n  region: {region_str}\n  signature_version2: false"
                    )
                })
            }
            PrometheusConfiguration::AzureBlobContainer => {}
            PrometheusConfiguration::ScalewayObjectStorage {
                bucket_name,
                region,
                endpoint,
                access_key,
                secret_key,
            } => {
                chart_info.values_string.push(ChartSetValue {
                    key: "objstoreConfig".to_string(),
                    value: format!(
                        "type: S3\nconfig:\n  bucket: {bucket_name}\n  endpoint: {endpoint}\n  region: {region}\n  signature_version2: false\n  access_key: {access_key}\n  secret_key: {secret_key}"
                    )
                })
            }
            PrometheusConfiguration::GcpCloudStorage {
                thanos_service_account_email,
                bucket_name,
            } => {
                chart_info.values.push(ChartSetValue {
                    key: r"storegateway.serviceAccount.annotations.iam\.gke\.io/gcp-service-account".to_string(),
                    value: thanos_service_account_email.clone(),
                });
                chart_info.values.push(ChartSetValue {
                    key: r"compactor.serviceAccount.annotations.iam\.gke\.io/gcp-service-account".to_string(),
                    value: thanos_service_account_email.clone(),
                });
                chart_info.values.push(ChartSetValue {
                    key: r"bucketweb.serviceAccount.annotations.iam\.gke\.io/gcp-service-account".to_string(),
                    value: thanos_service_account_email.clone(),
                });
                // INFO (ENG-1986) Pass the whole objstoreConfig as a string, as it is expected to be the content of a generated secret
                chart_info.values_string.push(ChartSetValue {
                    key: "objstoreConfig".to_string(),
                    value: format!("type: GCS\nconfig:\n  bucket: {bucket_name}"),
                })
            }
            PrometheusConfiguration::NotInstalled => {}
        }

        let common_chart = CommonChart::new(chart_info, None, None);
        Ok(common_chart)
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::models::aws::AwsStorageType;
    use crate::helm::{HelmAction, HelmChartNamespaces};
    use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::PrometheusConfiguration;

    use crate::infrastructure::helm_charts::thanos::ThanosChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
    use crate::infrastructure::models::kubernetes::Kind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn thanos_chart_directory_exists_test() {
        // setup:
        let chart = ThanosChart::new(
            HelmAction::Deploy,
            None,
            HelmChartNamespaces::Prometheus,
            None,
            PrometheusConfiguration::AwsS3 {
                region: AwsRegion::EuWest3,
                bucket_name: "s3_bucket_name".to_string(),
                endpoint: "endpoint_name".to_string(),
                aws_iam_prometheus_role_arn: "prometheus_role_arn".to_string(),
            },
            AwsStorageType::GP2.to_k8s_storage_class(),
            None,
            None,
            None,
            None,
            false,
            true,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            ThanosChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn kube_state_metrics_chart_values_file_exists_test() {
        // setup:
        let chart = ThanosChart::new(
            HelmAction::Deploy,
            None,
            HelmChartNamespaces::Prometheus,
            None,
            PrometheusConfiguration::AwsS3 {
                region: AwsRegion::EuWest3,
                bucket_name: "s3_bucket_name".to_string(),
                endpoint: "endpoint_name".to_string(),
                aws_iam_prometheus_role_arn: "prometheus_role_arn".to_string(),
            },
            AwsStorageType::GP2.to_k8s_storage_class(),
            None,
            None,
            None,
            None,
            false,
            true,
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
                ThanosChart::chart_name(),
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
    fn thanos_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = ThanosChart::new(
            HelmAction::Deploy,
            None,
            HelmChartNamespaces::Prometheus,
            None,
            PrometheusConfiguration::AwsS3 {
                region: AwsRegion::EuWest3,
                bucket_name: "s3_bucket_name".to_string(),
                endpoint: "endpoint_name".to_string(),
                aws_iam_prometheus_role_arn: "prometheus_role_arn".to_string(),
            },
            AwsStorageType::GP2.to_k8s_storage_class(),
            None,
            None,
            None,
            None,
            false,
            true,
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
                    ThanosChart::chart_name()
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
