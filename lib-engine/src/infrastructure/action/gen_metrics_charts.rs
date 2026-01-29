use crate::environment::models::ToCloudProviderFormat;
use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction, HelmChartNamespaces};
use crate::infrastructure::action::azure::helm_charts::AksChartsConfigPrerequisites;
use crate::infrastructure::action::eks::helm_charts::EksChartsConfigPrerequisites;
use crate::infrastructure::action::gke::helm_charts::GkeChartsConfigPrerequisites;
use crate::infrastructure::action::metrics_resource_profile::ResourceProfile;
use crate::infrastructure::action::scaleway::helm_charts::KapsuleChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::alert_config_chart::AlertConfigChart;
use crate::infrastructure::helm_charts::beyla_chart::BeylaChart;
use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::{
    KubePrometheusStackChart, PrometheusConfiguration,
};
use crate::infrastructure::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
use crate::infrastructure::helm_charts::prometheus_operator_crds::PrometheusOperatorCrdsChart;
use crate::infrastructure::helm_charts::thanos::ThanosChart;
use crate::infrastructure::helm_charts::yace_chart::YaceChart;
use crate::infrastructure::models::kubernetes::aws::AwsStorageType;
use crate::infrastructure::models::kubernetes::azure::AzureStorageType;
use crate::infrastructure::models::kubernetes::gcp::GcpStorageType;
use crate::infrastructure::models::kubernetes::scaleway::ScwStorageType;
use crate::io_models::metrics::{
    AlertManagerConfig, CloudWatchExporterConfig, MetricsConfiguration, MetricsParameters,
};
use crate::io_models::models::CustomerHelmChartsOverride;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

/// Type alias for the chart override function to reduce verbosity
pub type ChartOverrideFn = Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>;

/// Macro to reduce boilerplate when accessing a field from all CloudProviderMetricsConfig variants
macro_rules! access_field_from_all_configs {
    ($self:ident, $field:ident) => {
        match $self {
            CloudProviderMetricsConfig::Eks(cfg) => &cfg.$field,
            CloudProviderMetricsConfig::Gke(cfg) => &cfg.$field,
            CloudProviderMetricsConfig::Kapsule(cfg) => &cfg.$field,
            CloudProviderMetricsConfig::Aks(cfg) => &cfg.$field,
        }
    };
}

pub enum CloudProviderMetricsConfig<'a> {
    Eks(&'a EksChartsConfigPrerequisites),
    Gke(&'a GkeChartsConfigPrerequisites),
    Kapsule(&'a KapsuleChartsConfigPrerequisites),
    Aks(&'a AksChartsConfigPrerequisites),
}

impl CloudProviderMetricsConfig<'_> {
    pub fn prometheus_configuration(&self) -> PrometheusConfiguration {
        match self {
            Self::Eks(cfg) => {
                let region = cfg.region.to_cloud_provider_format();
                PrometheusConfiguration::AwsS3 {
                    region: cfg.region.clone(),
                    bucket_name: cfg.aws_s3_prometheus_bucket_name.to_string(),
                    aws_iam_prometheus_role_arn: cfg.aws_iam_eks_prometheus_role_arn.to_string(),
                    endpoint: format!("s3.{region}.amazonaws.com"),
                }
            }
            Self::Gke(cfg) => PrometheusConfiguration::GcpCloudStorage {
                thanos_service_account_email: cfg.thanos_service_account_email.clone(),
                bucket_name: cfg.prometheus_bucket_name.to_string(),
            },
            Self::Kapsule(cfg) => PrometheusConfiguration::ScalewayObjectStorage {
                bucket_name: cfg.prometheus_storage_config_scaleway_s3.to_string(),
                region: cfg.zone.region().to_string(),
                endpoint: Url::parse(&cfg.endpoint)
                    .ok()
                    .and_then(|url| url.host_str().map(|host| host.to_string()))
                    .unwrap_or_else(|| cfg.endpoint.clone()),
                access_key: cfg.access_key.clone(),
                secret_key: cfg.secret_key.clone(),
            },
            Self::Aks(cfg) => PrometheusConfiguration::AzureBlobContainer {
                thanos_client_id: cfg.thanos_client_id.clone(),
                thanos_storage_account: cfg.thanos_storage_account.clone(),
                thanos_container_name: cfg.thanos_container_name.clone(),
            },
        }
    }

    pub fn storage_class(&self) -> String {
        match self {
            Self::Eks(_) => AwsStorageType::GP2.to_k8s_storage_class(),
            Self::Gke(_) => GcpStorageType::Balanced.to_k8s_storage_class(),
            Self::Kapsule(_) => ScwStorageType::SbvSsd.to_k8s_storage_class(),
            Self::Aks(_) => AzureStorageType::StandardSSDZRS.to_k8s_storage_class(),
        }
    }

    pub fn is_karpenter_enabled(&self) -> bool {
        match self {
            Self::Eks(cfg) => cfg.is_karpenter_enabled,
            Self::Gke(_) => false,
            Self::Kapsule(_) => false,
            Self::Aks(_) => false,
        }
    }

    pub fn get_organization_long_id(&self) -> Uuid {
        match self {
            Self::Eks(cfg) => cfg.organization_long_id,
            Self::Gke(cfg) => cfg.organization_long_id,
            Self::Kapsule(cfg) => cfg.organization_long_id,
            Self::Aks(cfg) => cfg.organization_long_id,
        }
    }

    pub fn metrics_parameters(&self) -> Option<&MetricsParameters> {
        access_field_from_all_configs!(self, metrics_parameters).as_ref()
    }

    fn metrics_namespace(&self) -> &str {
        match self {
            CloudProviderMetricsConfig::Gke(_) | CloudProviderMetricsConfig::Aks(_) => "qovery",
            CloudProviderMetricsConfig::Eks(_) | CloudProviderMetricsConfig::Kapsule(_) => "prometheus",
        }
    }

    fn build_metrics_url(&self, service: &str, port: u16) -> String {
        format!("http://{}.{}.svc.cluster.local:{}", service, self.metrics_namespace(), port)
    }

    pub fn metrics_query_url_for_qovery_installation(&self) -> String {
        self.build_metrics_url("thanos-query", 9090)
    }

    pub fn metrics_prometheus_url_for_qovery_installation(&self) -> String {
        self.build_metrics_url("prometheus-operated", 9090)
    }

    pub fn metrics_alert_manager_url_for_qovery_installation(&self) -> String {
        self.build_metrics_url("alertmanager-operated", 9093)
    }

    pub fn is_cilium_compatible(&self) -> bool {
        match self {
            Self::Eks(_) => false,
            Self::Gke(_) => true,
            Self::Kapsule(_) => true,
            Self::Aks(_) => true,
        }
    }

    pub fn cluster_name(&self) -> String {
        access_field_from_all_configs!(self, cluster_name).clone()
    }

    /// Create YACE chart for AWS CloudWatch metrics export (EKS only)
    pub fn create_yace_chart(
        &self,
        chart_prefix_path: Option<&str>,
        cloudwatch_exporter_config: &CloudWatchExporterConfig,
        resource_profile: ResourceProfile,
    ) -> Option<CommonChart> {
        if let Self::Eks(eks_config) = self {
            let action = if cloudwatch_exporter_config.enabled {
                HelmAction::Deploy
            } else {
                HelmAction::Destroy
            };

            YaceChart::new(
                action,
                chart_prefix_path,
                HelmChartNamespaces::Qovery,
                eks_config.aws_iam_cloudwatch_exporter_role_arn.clone(),
                eks_config.region.to_cloud_provider_format().to_string(),
                eks_config.cluster_id.to_string(),
                resource_profile,
            )
            .to_common_helm_chart()
            .ok()
        } else {
            None
        }
    }

    /// Create Beyla chart for eBPF-based observability (non-Cilium providers only)
    pub fn create_beyla_chart(
        &self,
        chart_prefix_path: Option<&str>,
        cluster_name: &str,
        install_beyla: bool,
    ) -> Option<CommonChart> {
        if !self.is_cilium_compatible() {
            let action = if install_beyla {
                HelmAction::Deploy
            } else {
                HelmAction::Destroy
            };

            BeylaChart::new(action, chart_prefix_path, HelmChartNamespaces::Qovery, cluster_name)
                .to_common_helm_chart()
                .ok()
        } else {
            None
        }
    }
}

#[derive(Default)]
pub struct MetricsConfig {
    pub prometheus_operator_crds_chart: Option<CommonChart>,
    pub kube_prometheus_stack_chart: Option<CommonChart>,
    pub thanos_chart: Option<CommonChart>,
    pub prometheus_adapter_chart: Option<CommonChart>,
    pub beyla_chart: Option<CommonChart>,
    pub alert_config_chart: Option<CommonChart>,
    pub yace_chart: Option<CommonChart>,
    pub metrics_query_url: Option<String>,
    pub prometheus_service_url: Option<String>,
    pub alert_manager_service_url: Option<String>,
}

pub fn generate_metrics_config(
    provider_config: CloudProviderMetricsConfig,
    chart_prefix_path: Option<&str>,
    prometheus_internal_url: &str,
    prometheus_namespace: HelmChartNamespaces,
    get_chart_override_fn: ChartOverrideFn,
) -> Result<MetricsConfig, CommandError> {
    let metrics_configuration = provider_config.metrics_parameters().map(|it| it.config.clone());
    let cluster_name = provider_config.cluster_name();
    let organization_id = provider_config.get_organization_long_id();

    match metrics_configuration {
        Some(MetricsConfiguration::MetricsInstalledByQovery {
            install_prometheus_adapter,
            enable_redundancy,
            beyla_config,
            alert_config,
            resource_profile,
            cloudwatch_exporter_config,
        }) => generate_charts_installed_by_qovery(
            HelmAction::Deploy,
            install_prometheus_adapter,
            chart_prefix_path,
            provider_config,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
            &cluster_name,
            enable_redundancy,
            beyla_config.is_some_and(|config| config.enabled),
            alert_config,
            resource_profile,
            cloudwatch_exporter_config,
            organization_id,
        ),
        None => generate_charts_installed_by_qovery(
            HelmAction::Destroy,
            false, // we force an uninstallation for prometheus adapter
            chart_prefix_path,
            provider_config,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
            &cluster_name,
            None,
            false,
            None,
            ResourceProfile::default(), // Use default profile for destroy action
            CloudWatchExporterConfig::default(),
            organization_id,
        ),
        Some(_) => Ok(MetricsConfig {
            prometheus_operator_crds_chart: None,
            kube_prometheus_stack_chart: None,
            thanos_chart: None,
            prometheus_adapter_chart: None,
            beyla_chart: None,
            alert_config_chart: None,
            yace_chart: None,
            metrics_query_url: None,
            prometheus_service_url: None,
            alert_manager_service_url: None,
        }),
    }
}

fn generate_charts_installed_by_qovery(
    helm_action: HelmAction,
    install_prometheus_adapter: bool,
    chart_prefix_path: Option<&str>,
    provider_config: CloudProviderMetricsConfig,
    prometheus_internal_url: &str,
    prometheus_namespace: HelmChartNamespaces,
    get_chart_override_fn: ChartOverrideFn,
    cluster_name: &str,
    enable_redundancy: Option<bool>,
    install_beyla: bool,
    alert_config: Option<AlertManagerConfig>,
    resource_profile: ResourceProfile,
    cloudwatch_exporter_config: CloudWatchExporterConfig,
    organization_id: Uuid,
) -> Result<MetricsConfig, CommandError> {
    // TODO (ENG-1986) ATM we can't install prometheus operator crds systematically, as some clients may have already installed some versions on their side
    // Prometheus CRDs
    let prometheus_operator_crds_chart = match helm_action {
        HelmAction::Deploy => Some(
            PrometheusOperatorCrdsChart::new(chart_prefix_path, prometheus_namespace.clone()).to_common_helm_chart()?,
        ),
        HelmAction::Destroy => None,
    };

    let enable_redundancy = enable_redundancy.unwrap_or(true);

    // Kube Prometheus Stack
    let kube_prometheus_stack_chart = KubePrometheusStackChart::new(
        helm_action.clone(),
        chart_prefix_path,
        provider_config.storage_class(),
        prometheus_internal_url.to_string(),
        prometheus_namespace.clone(),
        provider_config.prometheus_configuration(),
        get_chart_override_fn.clone(),
        false,
        provider_config.is_karpenter_enabled(),
        enable_redundancy,
        alert_config.clone(),
        resource_profile,
    )
    .to_common_helm_chart()?;

    // Thanos
    let thanos_chart = ThanosChart::new(
        helm_action.clone(),
        chart_prefix_path,
        prometheus_namespace.clone(),
        None,
        provider_config.prometheus_configuration(),
        provider_config.storage_class(),
        None,
        None,
        None,
        None,
        provider_config.is_karpenter_enabled(),
        enable_redundancy,
        resource_profile,
    )
    .to_common_helm_chart()?;

    // Prometheus Adapter
    let prometheus_adapter_helm_action = match install_prometheus_adapter {
        true => HelmAction::Deploy,
        false => HelmAction::Destroy,
    };
    let prometheus_adapter_chart = PrometheusAdapterChart::new(
        prometheus_adapter_helm_action,
        chart_prefix_path,
        prometheus_internal_url.to_string(),
        prometheus_namespace.clone(),
        get_chart_override_fn.clone(),
        true,
        provider_config.is_karpenter_enabled(),
        resource_profile,
    )
    .to_common_helm_chart()?;

    // Grafana Beyla. only for EKS.
    let beyla_chart = provider_config.create_beyla_chart(chart_prefix_path, cluster_name, install_beyla);

    // Alert Config
    let alert_config_chart = AlertConfigChart::new(
        helm_action.clone(),
        prometheus_namespace.clone(),
        chart_prefix_path,
        cluster_name,
        alert_config,
        organization_id,
    )
    .to_common_helm_chart()?;

    // YACE (AWS Cloud Watch exporter)
    let yace_chart =
        provider_config.create_yace_chart(chart_prefix_path, &cloudwatch_exporter_config, resource_profile);

    // Generate service URLs only on Deploy, not on Destroy
    let (metrics_query_url, prometheus_service_url, alert_manager_service_url) = match helm_action {
        HelmAction::Deploy => (
            Some(provider_config.metrics_query_url_for_qovery_installation()),
            Some(provider_config.metrics_prometheus_url_for_qovery_installation()),
            Some(provider_config.metrics_alert_manager_url_for_qovery_installation()),
        ),
        HelmAction::Destroy => (None, None, None),
    };

    Ok(MetricsConfig {
        prometheus_operator_crds_chart,
        kube_prometheus_stack_chart: Some(kube_prometheus_stack_chart),
        thanos_chart: Some(thanos_chart),
        prometheus_adapter_chart: Some(prometheus_adapter_chart),
        beyla_chart,
        alert_config_chart: Some(alert_config_chart),
        yace_chart,
        metrics_query_url,
        prometheus_service_url,
        alert_manager_service_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::models::third_parties::LetsEncryptConfig;
    use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
    use crate::infrastructure::models::dns_provider::qoverydns::QoveryDnsConfig;
    use crate::infrastructure::models::kubernetes::KubernetesVersion;
    use crate::infrastructure::models::kubernetes::aws::Options;
    use crate::infrastructure::models::kubernetes::gcp::VpcMode;
    use crate::infrastructure::models::kubernetes::keda::{KedaAvailability, KedaResourceProfile};
    use crate::io_models::engine_location::EngineLocation;
    use crate::io_models::models::{StorageClass, VpcQoveryNetworkMode};
    use std::sync::Arc;
    use time::Time;

    const KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_33 {
        prefix: None,
        patch: None,
        suffix: None,
    };

    #[test]
    fn test_metrics_query_url_on_deploy() {
        let helm_action = HelmAction::Deploy;
        let install_prometheus_adapter = true;
        let chart_prefix_path = Some("charts/");
        let prometheus_internal_url = "http://prometheus.internal";
        let prometheus_namespace = HelmChartNamespaces::Prometheus;
        let config = create_eks_chart_config();
        let provider_config = CloudProviderMetricsConfig::Eks(&config);

        let get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> = Arc::new(|_| None);

        let result = generate_charts_installed_by_qovery(
            helm_action,
            install_prometheus_adapter,
            chart_prefix_path,
            provider_config,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
            "cluster-name",
            None,
            true,
            None,
            ResourceProfile::default(),
            CloudWatchExporterConfig::default(),
            Uuid::new_v4(),
        );

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(
            config.metrics_query_url,
            Some("http://thanos-query.prometheus.svc.cluster.local:9090".to_string())
        );
    }

    #[test]
    fn test_metrics_query_url_on_destroy() {
        let helm_action = HelmAction::Destroy;
        let install_prometheus_adapter = true;
        let chart_prefix_path = Some("charts/");
        let config = create_eks_chart_config();
        let provider_config = CloudProviderMetricsConfig::Eks(&config);

        let prometheus_internal_url = "http://prometheus.internal";
        let prometheus_namespace = HelmChartNamespaces::Prometheus;

        let get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> = Arc::new(|_| None);

        let result = generate_charts_installed_by_qovery(
            helm_action,
            install_prometheus_adapter,
            chart_prefix_path,
            provider_config,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
            "cluster-name",
            None,
            true,
            None,
            ResourceProfile::default(),
            CloudWatchExporterConfig::default(),
            Uuid::new_v4(),
        );

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.metrics_query_url, None);
    }

    #[test]
    fn test_metrics_urls_for_eks() {
        let config = create_eks_chart_config();
        let provider_config = CloudProviderMetricsConfig::Eks(&config);

        assert_eq!(
            provider_config.metrics_query_url_for_qovery_installation(),
            "http://thanos-query.prometheus.svc.cluster.local:9090"
        );
        assert_eq!(
            provider_config.metrics_prometheus_url_for_qovery_installation(),
            "http://prometheus-operated.prometheus.svc.cluster.local:9090"
        );
        assert_eq!(
            provider_config.metrics_alert_manager_url_for_qovery_installation(),
            "http://alertmanager-operated.prometheus.svc.cluster.local:9093"
        );
    }

    #[test]
    fn test_metrics_urls_for_gke() {
        let config = create_gke_chart_config();
        let provider_config = CloudProviderMetricsConfig::Gke(&config);

        assert_eq!(
            provider_config.metrics_query_url_for_qovery_installation(),
            "http://thanos-query.qovery.svc.cluster.local:9090"
        );
        assert_eq!(
            provider_config.metrics_prometheus_url_for_qovery_installation(),
            "http://prometheus-operated.qovery.svc.cluster.local:9090"
        );
        assert_eq!(
            provider_config.metrics_alert_manager_url_for_qovery_installation(),
            "http://alertmanager-operated.qovery.svc.cluster.local:9093"
        );
    }

    /// Helper to create a minimal QoveryDns configuration for tests
    fn create_test_dns_config() -> crate::infrastructure::models::dns_provider::DnsProviderConfiguration {
        crate::infrastructure::models::dns_provider::DnsProviderConfiguration::QoveryDns(QoveryDnsConfig {
            api_url: Url::parse("http://test.com").unwrap(),
            api_key: String::new(),
            api_url_scheme_and_domain: String::new(),
            api_url_port: String::new(),
        })
    }

    fn create_gke_chart_config() -> GkeChartsConfigPrerequisites {
        use crate::infrastructure::models::kubernetes::gcp::GkeOptions;

        GkeChartsConfigPrerequisites {
            organization_id: String::new(),
            organization_long_id: Default::default(),
            cluster_id: String::new(),
            cluster_long_id: Default::default(),
            cluster_name: String::new(),
            cluster_creation_date: Default::default(),
            ff_log_history_enabled: false,
            managed_dns_root_domain_helm_format: String::new(),
            lets_encrypt_config: LetsEncryptConfig::new("a".to_string(), true),
            dns_provider_config: create_test_dns_config(),
            loki_logging_service_account_email: String::new(),
            logs_bucket_name: String::new(),
            metrics_parameters: None,
            infra_options: GkeOptions {
                qovery_api_url: String::new(),
                qovery_grpc_url: String::new(),
                qovery_engine_url: String::new(),
                jwt_token: String::new(),
                qovery_ssh_key: String::new(),
                user_ssh_keys: vec![],
                grafana_admin_user: String::new(),
                grafana_admin_password: String::new(),
                qovery_engine_location: EngineLocation::ClientSide,
                vpc_mode: VpcMode::Automatic {
                    custom_cluster_ipv4_cidr_block: None,
                    custom_services_ipv4_cidr_block: None,
                },
                vpc_qovery_network_mode: None,
                cluster_maintenance_start_time: Time::MIDNIGHT,
                cluster_maintenance_end_time: None,
                tls_email_report: String::new(),
                metrics_parameters: None,
                keda_parameters: None,
            },
            cluster_advanced_settings: Default::default(),
            customer_helm_charts_override: None,
            thanos_service_account_email: String::new(),
            prometheus_bucket_name: String::new(),
            is_keda_enabled: false,
            keda_resource_profile: Default::default(),
            keda_availability: Default::default(),
            gcp_keda_operator_service_account_email: None,
            gcp_keda_metrics_server_service_account_email: None,
        }
    }

    fn create_eks_chart_config() -> EksChartsConfigPrerequisites {
        EksChartsConfigPrerequisites {
            organization_id: String::new(),
            organization_long_id: Default::default(),
            cluster_id: String::new(),
            cluster_long_id: Default::default(),
            cluster_creation_date: Default::default(),
            region: AwsRegion::UsEast1,
            kubernetes_version: KUBERNETES_VERSION,
            cluster_name: String::new(),
            cpu_architectures: vec![],
            cloud_provider: String::new(),
            qovery_engine_location: EngineLocation::ClientSide,
            ff_log_history_enabled: false,
            ff_grafana_enabled: false,
            managed_dns_helm_format: String::new(),
            managed_dns_resolvers_terraform_format: String::new(),
            managed_dns_root_domain_helm_format: String::new(),
            lets_encrypt_config: LetsEncryptConfig::new("a".to_string(), true),
            dns_provider_config: create_test_dns_config(),
            alb_controller_already_deployed: false,
            kubernetes_version_upgrade_requested: false,
            infra_options: Options {
                vpc_qovery_network_mode: VpcQoveryNetworkMode::WithNatGateways,
                qovery_engine_location: EngineLocation::ClientSide,
                ec2_zone_a_subnet_blocks: vec![],
                ec2_zone_b_subnet_blocks: vec![],
                ec2_zone_c_subnet_blocks: vec![],
                eks_zone_a_subnet_blocks: vec![],
                eks_zone_b_subnet_blocks: vec![],
                eks_zone_c_subnet_blocks: vec![],
                rds_zone_a_subnet_blocks: vec![],
                rds_zone_b_subnet_blocks: vec![],
                rds_zone_c_subnet_blocks: vec![],
                documentdb_zone_a_subnet_blocks: vec![],
                documentdb_zone_b_subnet_blocks: vec![],
                documentdb_zone_c_subnet_blocks: vec![],
                elasticache_zone_a_subnet_blocks: vec![],
                elasticache_zone_b_subnet_blocks: vec![],
                elasticache_zone_c_subnet_blocks: vec![],
                vpc_cidr_block: String::new(),
                eks_cidr_subnet: String::new(),
                ec2_cidr_subnet: String::new(),
                vpc_custom_routing_table: vec![],
                rds_cidr_subnet: String::new(),
                documentdb_cidr_subnet: String::new(),
                elasticache_cidr_subnet: String::new(),
                qovery_api_url: String::new(),
                qovery_grpc_url: String::new(),
                qovery_engine_url: String::new(),
                jwt_token: String::new(),
                grafana_admin_user: String::new(),
                grafana_admin_password: String::new(),
                qovery_ssh_key: String::new(),
                user_ssh_keys: vec![],
                tls_email_report: String::new(),
                user_provided_network: None,
                aws_addon_cni_version_override: None,
                aws_addon_kube_proxy_version_override: None,
                aws_addon_ebs_csi_version_override: None,
                aws_addon_coredns_version_override: None,
                ec2_exposed_port: None,
                karpenter_parameters: None,
                keda_parameters: None,
                metrics_parameters: None,
            },
            cluster_advanced_settings: Default::default(),
            is_karpenter_enabled: false,
            karpenter_parameters: None,
            is_keda_enabled: false,
            keda_resource_profile: KedaResourceProfile::Normal,
            keda_availability: KedaAvailability::Normal,
            aws_iam_keda_operator_role_arn: None,
            aws_iam_keda_metrics_server_role_arn: None,
            aws_iam_eks_user_mapper_role_arn: String::new(),
            aws_iam_cluster_autoscaler_role_arn: String::new(),
            aws_iam_cloudwatch_role_arn: String::new(),
            aws_iam_loki_role_arn: String::new(),
            aws_s3_loki_bucket_name: String::new(),
            loki_storage_config_aws_s3: String::new(),
            metrics_parameters: None,
            aws_iam_eks_prometheus_role_arn: String::new(),
            aws_s3_prometheus_bucket_name: String::new(),
            karpenter_controller_aws_role_arn: String::new(),
            cluster_security_group_id: String::new(),
            aws_iam_alb_controller_arn: String::new(),
            customer_helm_charts_override: None,
            aws_iam_cloudwatch_exporter_role_arn: None,
            kubernetes_storage_class_fast_ssd: StorageClass(AwsStorageType::GP2.to_k8s_storage_class()),
        }
    }
}
