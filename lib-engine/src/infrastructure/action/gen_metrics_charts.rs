use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::aws::AwsStorageType;
use crate::environment::models::azure::AzureStorageType;
use crate::environment::models::gcp::GcpStorageType;
use crate::environment::models::scaleway::ScwStorageType;
use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction, HelmChartNamespaces};
use crate::infrastructure::action::azure::helm_charts::AksChartsConfigPrerequisites;
use crate::infrastructure::action::eks::helm_charts::EksChartsConfigPrerequisites;
use crate::infrastructure::action::gke::helm_charts::GkeChartsConfigPrerequisites;
use crate::infrastructure::action::scaleway::helm_charts::KapsuleChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::{
    KubePrometheusStackChart, PrometheusConfiguration,
};
use crate::infrastructure::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
use crate::infrastructure::helm_charts::prometheus_operator_crds::PrometheusOperatorCrdsChart;
use crate::infrastructure::helm_charts::thanos::ThanosChart;
use crate::io_models::metrics::{MetricsConfiguration, MetricsParameters};
use crate::io_models::models::CustomerHelmChartsOverride;
use std::sync::Arc;
use url::Url;

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
            Self::Aks(_cfg) => PrometheusConfiguration::AzureBlobContainer {},
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
            Self::Aks(_) => true,
        }
    }

    pub fn metrics_parameters(&self) -> Option<&MetricsParameters> {
        match self {
            Self::Eks(cfg) => cfg.metrics_parameters.as_ref(),
            Self::Gke(cfg) => cfg.metrics_parameters.as_ref(),
            Self::Kapsule(cfg) => cfg.metrics_parameters.as_ref(),
            Self::Aks(cfg) => cfg.metrics_parameters.as_ref(),
        }
    }

    pub fn metrics_query_url_for_qovery_installation(&self) -> String {
        match self {
            CloudProviderMetricsConfig::Eks(_)
            | CloudProviderMetricsConfig::Kapsule(_)
            | CloudProviderMetricsConfig::Aks(_) => "http://thanos-query.prometheus.svc.cluster.local:9090".to_string(),
            CloudProviderMetricsConfig::Gke(_) => "http://thanos-query.qovery.svc.cluster.local:9090".to_string(),
        }
    }
}

#[derive(Default)]
pub struct MetricsConfig {
    pub prometheus_operator_crds_chart: Option<CommonChart>,
    pub kube_prometheus_stack_chart: Option<CommonChart>,
    pub thanos_chart: Option<CommonChart>,
    pub prometheus_adapter_chart: Option<CommonChart>,
    pub metrics_query_url: Option<String>,
}

pub fn generate_metrics_config(
    provider_config: CloudProviderMetricsConfig,
    chart_prefix_path: Option<&str>,
    prometheus_internal_url: &str,
    prometheus_namespace: HelmChartNamespaces,
    get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
    cluster_id: &str,
) -> Result<MetricsConfig, CommandError> {
    let metrics_configuration = provider_config.metrics_parameters().map(|it| it.config.clone());

    match metrics_configuration {
        Some(MetricsConfiguration::MetricsInstalledByQovery {
            install_prometheus_adapter,
            enable_redundancy,
        }) => generate_charts_installed_by_qovery(
            HelmAction::Deploy,
            install_prometheus_adapter,
            chart_prefix_path,
            provider_config,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
            cluster_id,
            enable_redundancy,
        ),
        None => generate_charts_installed_by_qovery(
            HelmAction::Destroy,
            false, // we force a desinstall for prometheus adapter
            chart_prefix_path,
            provider_config,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
            cluster_id,
            None,
        ),
        Some(_) => Ok(MetricsConfig {
            prometheus_operator_crds_chart: None,
            kube_prometheus_stack_chart: None,
            thanos_chart: None,
            prometheus_adapter_chart: None,
            metrics_query_url: None,
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
    get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
    _cluster_id: &str,
    enable_redundancy: Option<bool>,
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
        prometheus_namespace,
        get_chart_override_fn.clone(),
        true,
        provider_config.is_karpenter_enabled(),
    )
    .to_common_helm_chart()?;

    Ok(MetricsConfig {
        prometheus_operator_crds_chart,
        kube_prometheus_stack_chart: Some(kube_prometheus_stack_chart),
        thanos_chart: Some(thanos_chart),
        prometheus_adapter_chart: Some(prometheus_adapter_chart),
        metrics_query_url: match helm_action {
            HelmAction::Deploy => Some(provider_config.metrics_query_url_for_qovery_installation()),
            HelmAction::Destroy => None,
        },
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
    use crate::io_models::engine_location::EngineLocation;
    use crate::io_models::models::VpcQoveryNetworkMode;
    use std::sync::Arc;

    const KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_32 {
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
            "none",
            None,
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
            "none",
            None,
        );

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.metrics_query_url, None);
    }

    fn create_eks_chart_config() -> EksChartsConfigPrerequisites {
        EksChartsConfigPrerequisites {
            organization_id: "".to_string(),
            organization_long_id: Default::default(),
            cluster_id: "".to_string(),
            cluster_long_id: Default::default(),
            cluster_creation_date: Default::default(),
            region: AwsRegion::UsEast1,
            kubernetes_version: KUBERNETES_VERSION,
            cluster_name: "".to_string(),
            cpu_architectures: vec![],
            cloud_provider: "".to_string(),
            qovery_engine_location: EngineLocation::ClientSide,
            ff_log_history_enabled: false,
            ff_grafana_enabled: false,
            managed_dns_helm_format: "".to_string(),
            managed_dns_resolvers_terraform_format: "".to_string(),
            managed_dns_root_domain_helm_format: "".to_string(),
            lets_encrypt_config: LetsEncryptConfig::new("a".to_string(), true),
            dns_provider_config: crate::infrastructure::models::dns_provider::DnsProviderConfiguration::QoveryDns(
                QoveryDnsConfig {
                    api_url: Url::parse("http://test.com").unwrap(),
                    api_key: "".to_string(),
                    api_url_scheme_and_domain: "".to_string(),
                    api_url_port: "".to_string(),
                },
            ),
            alb_controller_already_deployed: false,
            kubernetes_version_upgrade_requested: false,
            infra_options: Options {
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
                fargate_profile_zone_a_subnet_blocks: vec![],
                fargate_profile_zone_b_subnet_blocks: vec![],
                fargate_profile_zone_c_subnet_blocks: vec![],
                eks_zone_a_nat_gw_for_fargate_subnet_blocks_public: vec![],
                vpc_qovery_network_mode: VpcQoveryNetworkMode::WithNatGateways,
                vpc_cidr_block: "".to_string(),
                eks_cidr_subnet: "".to_string(),
                ec2_cidr_subnet: "".to_string(),
                vpc_custom_routing_table: vec![],
                rds_cidr_subnet: "".to_string(),
                documentdb_cidr_subnet: "".to_string(),
                elasticache_cidr_subnet: "".to_string(),
                qovery_api_url: "".to_string(),
                qovery_grpc_url: "".to_string(),
                qovery_engine_url: "".to_string(),
                jwt_token: "".to_string(),
                qovery_engine_location: EngineLocation::ClientSide,
                grafana_admin_user: "".to_string(),
                grafana_admin_password: "".to_string(),
                qovery_ssh_key: "".to_string(),
                user_ssh_keys: vec![],
                tls_email_report: "".to_string(),
                user_provided_network: None,
                aws_addon_cni_version_override: None,
                aws_addon_kube_proxy_version_override: None,
                aws_addon_ebs_csi_version_override: None,
                aws_addon_coredns_version_override: None,
                ec2_exposed_port: None,
                karpenter_parameters: None,
                metrics_parameters: None,
            },
            cluster_advanced_settings: Default::default(),
            is_karpenter_enabled: false,
            karpenter_parameters: None,
            aws_iam_eks_user_mapper_role_arn: "".to_string(),
            aws_iam_cluster_autoscaler_role_arn: "".to_string(),
            aws_iam_cloudwatch_role_arn: "".to_string(),
            aws_iam_loki_role_arn: "".to_string(),
            aws_s3_loki_bucket_name: "".to_string(),
            loki_storage_config_aws_s3: "".to_string(),
            metrics_parameters: None,
            aws_iam_eks_prometheus_role_arn: "".to_string(),
            aws_s3_prometheus_bucket_name: "".to_string(),
            karpenter_controller_aws_role_arn: "".to_string(),
            cluster_security_group_id: "".to_string(),
            aws_iam_alb_controller_arn: "".to_string(),
            customer_helm_charts_override: None,
        }
    }
}
