use std::sync::Arc;

use crate::environment::models::ToCloudProviderFormat;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmAction, HelmChartError,
    HelmChartNamespaces, QoveryPriorityClass, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion,
    VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use crate::io_models::models::{CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use kube::Client;
use semver::Version;

pub type StorageClassName = String;

#[derive(Clone)]
pub struct AwsS3PrometheusChartConfiguration {
    pub region: String,
    pub bucket_name: String,
    pub endpoint: String,
    pub aws_iam_prometheus_role_arn: String,
}

#[derive(Clone)]
pub enum PrometheusConfiguration {
    AwsS3 {
        region: AwsRegion,
        bucket_name: String,
        endpoint: String,
        aws_iam_prometheus_role_arn: String,
    },
    ScalewayObjectStorage {
        bucket_name: String,
        region: String,
        endpoint: String,
        access_key: String,
        secret_key: String,
    },
    GcpCloudStorage {
        thanos_service_account_email: String,
        bucket_name: String,
    },
    NotInstalled,
}

pub struct KubePrometheusStackChart {
    action: HelmAction,
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    storage_class_name: StorageClassName,
    prometheus_object_bucket_configuration: PrometheusConfiguration,
    prometheus_internal_url: String,
    prometheus_namespace: HelmChartNamespaces,
    kubelet_service_monitor_resource_enabled: bool,
    customer_helm_chart_override: Option<CustomerHelmChartsOverride>,
    enable_vpa: bool,
    additional_chart_path: Option<HelmChartValuesFilePath>,
}

impl KubePrometheusStackChart {
    pub fn new(
        action: HelmAction,
        chart_prefix_path: Option<&str>,
        storage_class_name: StorageClassName,
        prometheus_internal_url: String,
        prometheus_namespace: HelmChartNamespaces,
        prometheus_object_bucket_configuration: PrometheusConfiguration,
        kubelet_service_monitor_resource_enabled: bool,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        enable_vpa: bool,
        karpenter_enabled: bool,
    ) -> Self {
        KubePrometheusStackChart {
            action,
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KubePrometheusStackChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KubePrometheusStackChart::chart_name(),
            ),
            storage_class_name,
            prometheus_object_bucket_configuration,
            prometheus_internal_url,
            prometheus_namespace,
            kubelet_service_monitor_resource_enabled,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            enable_vpa,
            additional_chart_path: match karpenter_enabled {
                true => Some(HelmChartValuesFilePath::new(
                    chart_prefix_path,
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "kube-prometheus-stack-with-karpenter".to_string(),
                )),
                false => None,
            },
        }
    }

    pub fn chart_name() -> String {
        "kube-prometheus-stack".to_string()
    }
}

impl ToCommonHelmChart for KubePrometheusStackChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values_files = vec![self.chart_values_path.to_string()];
        if let Some(additional_chart_path) = &self.additional_chart_path {
            values_files.push(additional_chart_path.to_string());
        }

        // thanos object storage configuration
        let mut object_storage_configs = match self.prometheus_object_bucket_configuration.clone() {
            PrometheusConfiguration::AwsS3 {
                region,
                bucket_name,
                // TODO (ENG-1986) To check if we really need this field
                endpoint: _,
                aws_iam_prometheus_role_arn,
            } => {
                let region_str = region.to_cloud_provider_format();
                vec![
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.region".to_string(),
                        value: region_str.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.bucket".to_string(),
                        value: bucket_name,
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.endpoint".to_string(),
                        value: format!("s3.{region_str}.amazonaws.com"),
                    },
                    ChartSetValue {
                        key: r"prometheus.serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: aws_iam_prometheus_role_arn,
                    },
                    // Make sure you use a correct signature version. Currently AWS requires signature v4, so it needs signature_version2: false.
                    // If you don’t specify it, you will get an Access Denied error. On the other hand, several S3 compatible APIs use signature_version2: true.
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.signature_version2"
                            .to_string(),
                        value: false.to_string(),
                    },
                ]
            }
            PrometheusConfiguration::NotInstalled => vec![],
            PrometheusConfiguration::ScalewayObjectStorage {
                bucket_name,
                region,
                endpoint,
                access_key,
                secret_key,
            } => vec![
                ChartSetValue {
                    key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.bucket".to_string(),
                    value: bucket_name,
                },
                ChartSetValue {
                    key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.region".to_string(),
                    value: region,
                },
                ChartSetValue {
                    key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.endpoint".to_string(),
                    value: endpoint,
                },
                ChartSetValue {
                    key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.access_key".to_string(),
                    value: access_key,
                },
                ChartSetValue {
                    key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.secret_key".to_string(),
                    value: secret_key,
                },
            ],
            PrometheusConfiguration::GcpCloudStorage {
                thanos_service_account_email,
                bucket_name,
            } => vec![
                ChartSetValue {
                    key: "prometheus.prometheusSpec.thanos.objectStorageConfig.secret.config.bucket".to_string(),
                    value: bucket_name,
                },
                ChartSetValue {
                    key: r"prometheus.serviceAccount.annotations.iam\.gke\.io/gcp-service-account".to_string(),
                    value: thanos_service_account_email.clone(),
                },
            ],
        };

        let mut common_chart = CommonChart {
            chart_info: ChartInfo {
                action: self.action.clone(),
                name: KubePrometheusStackChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.prometheus_namespace,
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(67, 3, 1)),
                // high timeout because on bootstrap, it's one of the biggest dependencies and on upgrade, it can takes time
                // to upgrade because of the CRD and the number of elements it has to deploy
                timeout_in_seconds: 480,
                // To check for upgrades: https://github.com/prometheus-community/helm-charts/tree/main/charts/kube-prometheus-stack
                values_files,
                values: vec![
                    // we should not enable crds because we are using the prometheus-operator-crds chart
                    ChartSetValue {
                        key: "crds.enabled".to_string(),
                        value: false.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.replicas".to_string(),
                        value: "2".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.storageSpec.volumeClaimTemplate.spec.storageClassName"
                            .to_string(),
                        value: self.storage_class_name.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.externalUrl".to_string(),
                        value: self.prometheus_internal_url.clone(),
                    },
                    ChartSetValue {
                        key: "kubelet.serviceMonitor.resource".to_string(),
                        value: self.kubelet_service_monitor_resource_enabled.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus-node-exporter.priorityClassName".to_string(),
                        value: QoveryPriorityClass::HighPriority.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.thanosService.enabled".to_string(),
                        value: "true".to_string(),
                    },
                ],
                yaml_files_content: match self.customer_helm_chart_override.clone() {
                    Some(x) => vec![x.to_chart_values_generated()],
                    None => vec![],
                },
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KubePrometheusStackChartChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "kube-prometheus-stack-operator".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(200)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(2000)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(384)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(4)),
                            ),
                        },
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "kube-state-metrics".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(50)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(200)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(64)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                            ),
                        },
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::DaemonSet,
                                "kube-prometheus-stack-prometheus-node-exporter".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(150)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(500)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(16)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(256)),
                            ),
                        },
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::StatefulSet,
                                "prometheus-kube-prometheus-stack-prometheus".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(1000)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(8)),
                            ),
                        },
                    ],
                )),
                false => None,
            },
        };

        common_chart.chart_info.values.append(&mut object_storage_configs);

        Ok(common_chart)
    }
}

#[derive(Clone)]
pub struct KubePrometheusStackChartChecker {}

impl KubePrometheusStackChartChecker {
    pub fn new() -> KubePrometheusStackChartChecker {
        KubePrometheusStackChartChecker {}
    }
}

impl Default for KubePrometheusStackChartChecker {
    fn default() -> Self {
        KubePrometheusStackChartChecker::new()
    }
}

impl ChartInstallationChecker for KubePrometheusStackChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1373): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::sync::Arc;

    use crate::helm::{HelmAction, HelmChartNamespaces};
    use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::{PrometheusConfiguration, StorageClassName};
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
    use crate::infrastructure::models::kubernetes::Kind;
    use crate::io_models::models::CustomerHelmChartsOverride;

    use super::KubePrometheusStackChart;

    fn get_prometheus_chart_override() -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
        Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> {
            Some(CustomerHelmChartsOverride {
                chart_name: KubePrometheusStackChart::chart_name(),
                chart_values: "".to_string(),
            })
        })
    }
    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn kube_prometheus_stack_chart_directory_exists_test() {
        // setup:
        let chart = KubePrometheusStackChart::new(
            HelmAction::Deploy,
            None,
            StorageClassName::new(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            PrometheusConfiguration::AwsS3 {
                region: AwsRegion::EuWest3,
                bucket_name: "whatever".to_string(),
                endpoint: "whatever".to_string(),
                aws_iam_prometheus_role_arn: "whatever".to_string(),
            },
            true,
            get_prometheus_chart_override(),
            false,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            KubePrometheusStackChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn kube_prometheus_stack_chart_values_file_exists_test() {
        // setup:
        let chart = KubePrometheusStackChart::new(
            HelmAction::Deploy,
            None,
            StorageClassName::new(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            PrometheusConfiguration::AwsS3 {
                region: AwsRegion::EuWest3,
                bucket_name: "whatever".to_string(),
                endpoint: "whatever".to_string(),
                aws_iam_prometheus_role_arn: "whatever".to_string(),
            },
            true,
            get_prometheus_chart_override(),
            false,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        for provider_kind in [Kind::Eks, Kind::Gke, Kind::ScwKapsule] {
            let chart_values_path = format!(
                "{}/lib/{}/bootstrap/chart_values/{}.yaml",
                current_directory
                    .to_str()
                    .expect("Impossible to convert current directory to string"),
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(provider_kind)
                ),
                KubePrometheusStackChart::chart_name(),
            );

            // execute
            let values_file = std::fs::File::open(&chart_values_path);

            // verify:
            assert!(
                values_file.is_ok(),
                "Chart values {} file should exist: `{chart_values_path}`",
                provider_kind
            );
        }
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn kube_prometheus_stack_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // TODO (ENG-1986) When adding Thanos to other cloud providers, add them here as well
        // execute:
        {
            let provider_kind = Kind::Eks;
            // setup:
            let chart = KubePrometheusStackChart::new(
                HelmAction::Deploy,
                None,
                "whatever".to_string(),
                "whatever".to_string(),
                HelmChartNamespaces::Prometheus,
                match provider_kind {
                    Kind::Eks => PrometheusConfiguration::AwsS3 {
                        region: AwsRegion::EuWest3,
                        bucket_name: "whatever".to_string(),
                        endpoint: "whatever".to_string(),
                        aws_iam_prometheus_role_arn: "whatever".to_string(),
                    },
                    Kind::ScwKapsule => PrometheusConfiguration::ScalewayObjectStorage {
                        bucket_name: "whatever".to_string(),
                        region: "whatever".to_string(),
                        endpoint: "whatever".to_string(),
                        access_key: "whatever".to_string(),
                        secret_key: "whatever".to_string(),
                    },
                    Kind::Gke => PrometheusConfiguration::GcpCloudStorage {
                        thanos_service_account_email: "whatever".to_string(),
                        bucket_name: "whatever".to_string(),
                    },
                    Kind::EksSelfManaged | Kind::GkeSelfManaged | Kind::ScwSelfManaged | Kind::OnPremiseSelfManaged => {
                        // TODO (ENG-1986) Not handled yet
                        PrometheusConfiguration::NotInstalled
                    }
                },
                true,
                get_prometheus_chart_override(),
                false,
                false,
            );
            let common_chart = chart.to_common_helm_chart().unwrap();

            let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
                common_chart.clone(),
                format!(
                    "/lib/{}/bootstrap/chart_values/{}.yaml",
                    get_helm_path_kubernetes_provider_sub_folder_name(
                        chart.chart_values_path.helm_path(),
                        HelmChartType::CloudProviderSpecific(provider_kind)
                    ),
                    KubePrometheusStackChart::chart_name()
                ),
            );

            // verify:
            assert!(
                missing_fields.is_none(),
                "Some fields are missing in values {} file, add those (make sure they still exist in chart values), fields: {}",
                provider_kind,
                missing_fields.unwrap_or_default().join(",")
            );
        }
    }
}
