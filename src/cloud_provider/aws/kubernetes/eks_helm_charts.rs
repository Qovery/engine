use crate::cloud_provider::aws::kubernetes::helm_charts::alb_controller::AwsLoadBalancerControllerChart;
use crate::cloud_provider::aws::kubernetes::helm_charts::karpenter::KarpenterChart;
use crate::cloud_provider::aws::kubernetes::{KarpenterParameters, Options};
use crate::cloud_provider::helm::{
    get_engine_helm_action_from_location, ChartInfo, ChartSetValue, CommonChart, HelmChart, HelmChartNamespaces,
    PriorityClass, QoveryPriorityClass, UpdateStrategy,
};
use crate::cloud_provider::helm_charts::coredns_config_chart::CoreDNSConfigChart;
use crate::cloud_provider::helm_charts::k8s_event_logger::K8sEventLoggerChart;
use crate::cloud_provider::helm_charts::nginx_ingress_chart::NginxIngressChart;
use crate::cloud_provider::helm_charts::promtail_chart::PromtailChart;
use crate::cloud_provider::helm_charts::qovery_shell_agent_chart::QoveryShellAgentChart;
use crate::cloud_provider::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::cloud_provider::helm_charts::vertical_pod_autoscaler::VpaChart;
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartResources, HelmChartResourcesConstraintType, HelmChartTimeout,
    ToCommonHelmChart,
};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::models::{
    CpuArchitecture, CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit,
    VpcQoveryNetworkMode,
};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::Kind;

use crate::dns_provider::DnsProviderConfiguration;
use crate::errors::CommandError;

use crate::cloud_provider::aws::kubernetes::helm_charts::aws_iam_eks_user_mapper_chart::{
    AwsIamEksUserMapperChart, GroupConfig, GroupConfigMapping, KarpenterConfig, SSOConfig,
};
use crate::cloud_provider::aws::kubernetes::helm_charts::aws_node_term_handler_chart::AwsNodeTermHandlerChart;
use crate::cloud_provider::aws::kubernetes::helm_charts::aws_ui_view_chart::AwsUiViewChart;
use crate::cloud_provider::aws::kubernetes::helm_charts::cluster_autoscaler_chart::ClusterAutoscalerChart;
use crate::cloud_provider::aws::kubernetes::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use crate::cloud_provider::aws::kubernetes::helm_charts::karpenter_crd::KarpenterCrdChart;
use crate::cloud_provider::aws::regions::AwsRegion;
use crate::cloud_provider::helm_charts::cert_manager_chart::CertManagerChart;
use crate::cloud_provider::helm_charts::cert_manager_config_chart::CertManagerConfigsChart;
use crate::cloud_provider::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::cloud_provider::helm_charts::grafana_chart::{
    CloudWatchConfig, GrafanaAdminUser, GrafanaChart, GrafanaDatasources,
};
use crate::cloud_provider::helm_charts::kube_prometheus_stack_chart::KubePrometheusStackChart;
use crate::cloud_provider::helm_charts::kube_state_metrics::KubeStateMetricsChart;
use crate::cloud_provider::helm_charts::loki_chart::{
    LokiChart, LokiObjectBucketConfiguration, S3LokiChartConfiguration,
};
use crate::cloud_provider::helm_charts::metrics_server_chart::MetricsServerChart;
use crate::cloud_provider::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
use crate::cloud_provider::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
use crate::cloud_provider::helm_charts::qovery_cluster_agent_chart::QoveryClusterAgentChart;
use crate::cloud_provider::helm_charts::qovery_pdb_infra_chart::QoveryPdbInfraChart;
use crate::cloud_provider::helm_charts::qovery_priority_class_chart::QoveryPriorityClassChart;
use crate::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::QoveryIdentifier;
use crate::models::aws::AwsStorageType;
use crate::models::domain::Domain;
use crate::models::third_parties::LetsEncryptConfig;
use crate::models::ToCloudProviderFormat;
use chrono::Duration;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::iter::FromIterator;
use std::path::Path;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsEksQoveryTerraformConfig {
    pub aws_account_id: String,
    pub aws_iam_eks_user_mapper_role_arn: String,
    pub aws_iam_cluster_autoscaler_role_arn: String,
    pub aws_iam_cloudwatch_role_arn: String,
    pub aws_iam_loki_role_arn: String,
    pub aws_s3_loki_bucket_name: String,
    pub loki_storage_config_aws_s3: String,
    pub karpenter_controller_aws_role_arn: String,
    pub cluster_security_group_id: String,
    pub aws_iam_alb_controller_arn: String,
}

pub struct EksChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub region: AwsRegion,
    pub cluster_name: String,
    pub cpu_architectures: Vec<CpuArchitecture>,
    pub cloud_provider: String,
    pub test_cluster: bool,
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    pub vpc_qovery_network_mode: VpcQoveryNetworkMode,
    pub qovery_engine_location: EngineLocation,
    pub ff_log_history_enabled: bool,
    pub ff_metrics_history_enabled: bool,
    pub ff_grafana_enabled: bool,
    pub managed_domain: Domain,
    pub managed_dns_name: String,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub managed_dns_root_domain_helm_format: String,
    pub external_dns_provider: String,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub dns_provider_config: DnsProviderConfiguration,
    // qovery options form json input
    pub infra_options: Options,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
    pub is_karpenter_enabled: bool,
    pub karpenter_parameters: Option<KarpenterParameters>,
}

pub fn eks_aws_helm_charts(
    qovery_terraform_config_file: &str,
    chart_config_prerequisites: &EksChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    _kubernetes_config: &Path,
    envs: &[(String, String)],
    qovery_api: &dyn QoveryApi,
    customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    domain: &Domain,
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
    let get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> =
        Arc::new(move |chart_name: String| -> Option<CustomerHelmChartsOverride> {
            match customer_helm_charts_override.clone() {
                Some(x) => x.get(&chart_name).map(|content| CustomerHelmChartsOverride {
                    chart_name: chart_name.to_string(),
                    chart_values: content.clone(),
                }),
                None => None,
            }
        });

    let qovery_terraform_config = get_qovery_terraform_config(qovery_terraform_config_file, envs)?;
    let chart_prefix = chart_prefix_path.unwrap_or("./");
    let chart_path = |x: &str| -> String { format!("{}/{}", &chart_prefix, x) };

    let prometheus_namespace = HelmChartNamespaces::Prometheus;
    let prometheus_internal_url = format!("http://prometheus-operated.{prometheus_namespace}.svc");
    let loki_namespace = HelmChartNamespaces::Logging;
    let loki_kube_dns_name = format!("loki.{loki_namespace}.svc:3100");

    // Qovery storage class
    let q_storage_class = QoveryStorageClassChart::new(
        chart_prefix_path,
        Kind::Aws,
        HashSet::from_iter(vec![
            QoveryStorageType::Ssd,
            QoveryStorageType::Hdd,
            QoveryStorageType::Cold,
            QoveryStorageType::Nvme,
        ]),
        HelmChartNamespaces::KubeSystem,
    )
    .to_common_helm_chart()?;

    // AWS IAM EKS user mapper
    let mut aws_iam_eks_user_mapper: Option<CommonChart> = None;
    if chart_config_prerequisites
        .cluster_advanced_settings
        .aws_iam_user_mapper_sso_enabled
        || chart_config_prerequisites
            .cluster_advanced_settings
            .aws_iam_user_mapper_group_enabled
        || chart_config_prerequisites.is_karpenter_enabled
    {
        aws_iam_eks_user_mapper = Some(
            AwsIamEksUserMapperChart::new(
                chart_prefix_path,
                "iam-eks-user-mapper".to_string(),
                qovery_terraform_config.aws_iam_eks_user_mapper_role_arn,
                match &chart_config_prerequisites
                    .cluster_advanced_settings
                    .aws_iam_user_mapper_group_enabled
                {
                    true => GroupConfig::Enabled {
                        group_config_mapping: vec![GroupConfigMapping {
                            iam_group_name: chart_config_prerequisites
                                .cluster_advanced_settings
                                .aws_iam_user_mapper_group_name
                                .as_ref()
                                .map(|v| v.to_string())
                                .unwrap_or_default(), // TODO(benjaminch): introduce a proper error
                            k8s_group_name: "system:masters".to_string(),
                        }],
                    },
                    false => GroupConfig::Disabled,
                },
                match &chart_config_prerequisites
                    .cluster_advanced_settings
                    .aws_iam_user_mapper_sso_enabled
                {
                    true => SSOConfig::Enabled {
                        sso_role_arn: chart_config_prerequisites
                            .cluster_advanced_settings
                            .aws_iam_user_mapper_sso_role_arn
                            .as_ref()
                            .map(|v| v.to_string())
                            .unwrap_or_default(), // TODO(benjaminch): introduce a proper error
                    },
                    false => SSOConfig::Disabled,
                },
                match chart_config_prerequisites.is_karpenter_enabled {
                    true => KarpenterConfig::Enabled {
                        aws_account_id: qovery_terraform_config.aws_account_id,
                        cluster_name: chart_config_prerequisites.cluster_name.clone(),
                    },
                    false => KarpenterConfig::Disabled,
                },
                Duration::seconds(30), // TODO(benjaminch): might be a parameter
                HelmChartResourcesConstraintType::ChartDefault,
            )
            .to_common_helm_chart()?,
        );
    }

    // AWS nodes term handler
    let aws_node_term_handler =
        AwsNodeTermHandlerChart::new(chart_prefix_path, chart_config_prerequisites.is_karpenter_enabled)
            .to_common_helm_chart()?;

    // AWS UI view
    let aws_ui_view = AwsUiViewChart::new(chart_prefix_path).to_common_helm_chart()?;

    // Vertical pod autoscaler
    let vpa = VpaChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartResourcesConstraintType::ChartDefault,
        true,
        HelmChartNamespaces::KubeSystem,
    )
    .to_common_helm_chart()?;

    // Karpenter
    let karpenter = KarpenterChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cluster_name.to_string(),
        qovery_terraform_config.karpenter_controller_aws_role_arn.clone(),
        chart_config_prerequisites.is_karpenter_enabled,
        false,
    )
    .to_common_helm_chart()?;

    // Karpenter CRD
    let karpenter_crd = KarpenterCrdChart::new(chart_prefix_path).to_common_helm_chart()?;

    let karpenter_with_monitoring = KarpenterChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cluster_name.to_string(),
        qovery_terraform_config.karpenter_controller_aws_role_arn,
        chart_config_prerequisites.is_karpenter_enabled,
        true,
    )
    .to_common_helm_chart()?;

    // Karpenter Configuration
    let karpenter_configuration = KarpenterConfigurationChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cluster_name.to_string(),
        chart_config_prerequisites.is_karpenter_enabled,
        qovery_terraform_config.cluster_security_group_id,
        &chart_config_prerequisites.cluster_id,
        chart_config_prerequisites.cluster_long_id,
        &chart_config_prerequisites.organization_id,
        chart_config_prerequisites.organization_long_id,
        chart_config_prerequisites.region.to_cloud_provider_format(),
        chart_config_prerequisites.karpenter_parameters.clone(),
        chart_config_prerequisites.infra_options.user_provided_network.as_ref(),
        chart_config_prerequisites.cluster_advanced_settings.pleco_resources_ttl,
    )
    .to_common_helm_chart()?;

    // Cluster autoscaler
    let cluster_autoscaler = ClusterAutoscalerChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cloud_provider.to_string(),
        chart_config_prerequisites.region.clone(),
        chart_config_prerequisites.cluster_name.to_string(),
        qovery_terraform_config.aws_iam_cluster_autoscaler_role_arn.to_string(),
        prometheus_namespace,
        chart_config_prerequisites.ff_metrics_history_enabled,
        chart_config_prerequisites.is_karpenter_enabled,
    )
    .to_common_helm_chart()?;

    // CoreDNS config
    let coredns_config = CoreDNSConfigChart::new(
        chart_prefix_path,
        false,
        chart_config_prerequisites.managed_dns_helm_format.to_string(),
        chart_config_prerequisites
            .managed_dns_resolvers_terraform_format
            .to_string(),
        HelmChartNamespaces::KubeSystem,
    );

    // ALB controller
    let aws_load_balancer_controller = AwsLoadBalancerControllerChart::new(
        chart_prefix_path,
        qovery_terraform_config.aws_iam_alb_controller_arn,
        chart_config_prerequisites.cluster_name.clone(),
    );

    // External DNS
    let external_dns = ExternalDNSChart::new(
        chart_prefix_path,
        chart_config_prerequisites.dns_provider_config.clone(),
        chart_config_prerequisites
            .managed_dns_root_domain_helm_format
            .to_string(),
        chart_config_prerequisites.cluster_id.to_string(),
        UpdateStrategy::RollingUpdate,
        true,
        HelmChartNamespaces::KubeSystem,
    )
    .to_common_helm_chart()?;

    // Promtail
    let promtail = match chart_config_prerequisites.ff_log_history_enabled {
        false => None,
        true => Some(
            PromtailChart::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                loki_kube_dns_name,
                get_chart_override_fn.clone(),
                true,
                HelmChartNamespaces::KubeSystem,
                PriorityClass::Default,
            )
            .to_common_helm_chart()?,
        ),
    };

    // Loki
    let loki = match chart_config_prerequisites.ff_log_history_enabled {
        false => None,
        true => Some(
            LokiChart::new(
                chart_prefix_path,
                // LokiEncryptionType::ServerSideEncryption,
                loki_namespace,
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .loki_log_retention_in_week,
                LokiObjectBucketConfiguration::S3(S3LokiChartConfiguration {
                    region: Some(chart_config_prerequisites.region.to_cloud_provider_format().to_string()), // TODO(benjaminch): region to be struct instead of String
                    bucketname: Some(qovery_terraform_config.aws_s3_loki_bucket_name),
                    s3_config: Some(qovery_terraform_config.loki_storage_config_aws_s3),
                    aws_iam_loki_role_arn: Some(qovery_terraform_config.aws_iam_loki_role_arn),
                    insecure: false,
                    use_path_style: false,
                }),
                get_chart_override_fn.clone(),
                true,
                None,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartTimeout::ChartDefault,
                chart_config_prerequisites.is_karpenter_enabled,
            )
            .to_common_helm_chart()?,
        ),
    };

    /* Example to delete an old install
    let old_prometheus_operator = PrometheusOperatorConfigChart {
        chart_info: ChartInfo {
            name: "prometheus-operator".to_string(),
            namespace: prometheus_namespace,
            action: HelmAction::Destroy,
            ..Default::default()
        },
    };*/

    // K8s Event Logger
    let k8s_event_logger =
        K8sEventLoggerChart::new(chart_prefix_path, true, HelmChartNamespaces::Qovery).to_common_helm_chart()?;

    // Kube prometheus stack
    let kube_prometheus_stack = match chart_config_prerequisites.ff_metrics_history_enabled {
        false => None,
        true => Some(
            KubePrometheusStackChart::new(
                chart_prefix_path,
                AwsStorageType::GP2.to_k8s_storage_class(),
                prometheus_internal_url.to_string(),
                prometheus_namespace,
                true,
                get_chart_override_fn.clone(),
                true,
            )
            .to_common_helm_chart()?,
        ),
    };

    // Prometheus adapter
    let prometheus_adapter = match chart_config_prerequisites.ff_metrics_history_enabled {
        false => None,
        true => Some(
            PrometheusAdapterChart::new(
                chart_prefix_path,
                prometheus_internal_url.clone(),
                prometheus_namespace,
                get_chart_override_fn.clone(),
                true,
            )
            .to_common_helm_chart()?,
        ),
    };

    let mut qovery_cert_manager_webhook: Option<CommonChart> = None;
    if let DnsProviderConfiguration::QoveryDns(qovery_dns_config) = &chart_config_prerequisites.dns_provider_config {
        qovery_cert_manager_webhook = Some(
            QoveryCertManagerWebhookChart::new(
                chart_prefix_path,
                qovery_dns_config.clone(),
                HelmChartResourcesConstraintType::ChartDefault,
                UpdateStrategy::RollingUpdate,
                HelmChartNamespaces::CertManager,
                HelmChartNamespaces::CertManager,
            )
            .to_common_helm_chart()?,
        );
    }

    // Metrics server
    let metrics_server = MetricsServerChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::ChartDefault,
        UpdateStrategy::RollingUpdate,
        true,
    )
    .to_common_helm_chart()?;

    // Kube state metrics
    let kube_state_metrics = match chart_config_prerequisites.ff_metrics_history_enabled {
        false => None,
        true => Some(
            KubeStateMetricsChart::new(
                chart_prefix_path,
                HelmChartNamespaces::Prometheus,
                true,
                get_chart_override_fn.clone(),
            )
            .to_common_helm_chart()?,
        ),
    };

    // Grafana chart
    let grafana = match chart_config_prerequisites.ff_grafana_enabled {
        false => None,
        true => Some(
            GrafanaChart::new(
                chart_prefix_path,
                GrafanaAdminUser::new(
                    chart_config_prerequisites.infra_options.grafana_admin_user.to_string(),
                    chart_config_prerequisites
                        .infra_options
                        .grafana_admin_password
                        .to_string(),
                ),
                GrafanaDatasources {
                    prometheus_internal_url,
                    loki_chart_name: LokiChart::chart_name(),
                    loki_namespace: loki_namespace.to_string(),
                    cloudwatch_config: Some(CloudWatchConfig::new(
                        chart_config_prerequisites.region.to_cloud_provider_format().to_string(), // TODO(benjaminch): region to be struct instead of String
                        qovery_terraform_config.aws_iam_cloudwatch_role_arn,
                    )),
                },
                AwsStorageType::GP2.to_k8s_storage_class(),
            )
            .to_common_helm_chart()?,
        ),
    };

    // Cert Manager chart
    let cert_manager = CertManagerChart::new(
        chart_prefix_path,
        chart_config_prerequisites.ff_metrics_history_enabled,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartResourcesConstraintType::ChartDefault,
        UpdateStrategy::RollingUpdate,
        get_chart_override_fn.clone(),
        true,
        HelmChartNamespaces::CertManager,
        HelmChartNamespaces::KubeSystem,
    )
    .to_common_helm_chart()?;

    // Cert Manager Configs
    let cert_manager_config = CertManagerConfigsChart::new(
        chart_prefix_path,
        &chart_config_prerequisites.lets_encrypt_config,
        &chart_config_prerequisites.dns_provider_config,
        chart_config_prerequisites.managed_dns_helm_format.to_string(),
        HelmChartNamespaces::CertManager,
    )
    .to_common_helm_chart()?;

    // Nginx ingress
    let nginx_ingress = NginxIngressChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::Constrained(HelmChartResources {
            request_cpu: KubernetesCpuResourceUnit::MilliCpu(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .nginx_vcpu_request_in_milli_cpu,
            ),
            request_memory: KubernetesMemoryResourceUnit::MebiByte(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .nginx_memory_request_in_mib,
            ),
            limit_cpu: KubernetesCpuResourceUnit::MilliCpu(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .nginx_vcpu_limit_in_milli_cpu,
            ),
            limit_memory: KubernetesMemoryResourceUnit::MebiByte(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .nginx_memory_limit_in_mib,
            ),
        }),
        HelmChartResourcesConstraintType::ChartDefault,
        chart_config_prerequisites.ff_metrics_history_enabled,
        get_chart_override_fn.clone(),
        domain.clone(),
        Kind::Aws,
        KubernetesKind::Eks,
        Some(
            chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_hpa_min_number_instances,
        ),
        Some(
            chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_hpa_max_number_instances,
        ),
        Some(
            chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_hpa_cpu_utilization_percentage_threshold,
        ),
        HelmChartNamespaces::NginxIngress,
        None,
        chart_config_prerequisites
            .cluster_advanced_settings
            .nginx_controller_enable_client_ip,
        chart_config_prerequisites
            .cluster_advanced_settings
            .nginx_controller_log_format_escaping
            .to_model(),
        chart_config_prerequisites
            .cluster_advanced_settings
            .aws_eks_enable_alb_controller,
    )
    .to_common_helm_chart()?;

    // Qovery cluster agent
    let cluster_agent = QoveryClusterAgentChart::new(
        chart_prefix_path,
        qovery_api
            .service_version(EngineServiceType::ClusterAgent)
            .map_err(|e| CommandError::new("cannot get cluster agent version".to_string(), Some(e.to_string()), None))?
            .as_str(),
        Url::parse(&chart_config_prerequisites.infra_options.qovery_grpc_url)
            .map_err(|e| CommandError::new("cannot parse GRPC url".to_string(), Some(e.to_string()), None))?,
        match chart_config_prerequisites.ff_log_history_enabled {
            true => Some(
                Url::parse("http://loki.logging.svc.cluster.local:3100")
                    .map_err(|e| CommandError::new("cannot parse Loki url".to_string(), Some(e.to_string()), None))?,
            ),
            false => None,
        },
        &chart_config_prerequisites.infra_options.jwt_token.clone(),
        QoveryIdentifier::new(chart_config_prerequisites.cluster_long_id),
        QoveryIdentifier::new(chart_config_prerequisites.organization_long_id),
        HelmChartResourcesConstraintType::ChartDefault,
        UpdateStrategy::RollingUpdate,
        true,
    )
    .to_common_helm_chart()?;

    // Qovery shell agent
    let qovery_shell_agent = QoveryShellAgentChart::new(
        chart_prefix_path,
        qovery_api
            .service_version(EngineServiceType::ShellAgent)
            .map_err(|e| CommandError::new("cannot get cluster agent version".to_string(), Some(e.to_string()), None))?
            .as_str(),
        chart_config_prerequisites.infra_options.jwt_token.clone(),
        QoveryIdentifier::new(chart_config_prerequisites.organization_long_id),
        QoveryIdentifier::new(chart_config_prerequisites.cluster_long_id),
        chart_config_prerequisites.infra_options.qovery_grpc_url.clone(),
        HelmChartResourcesConstraintType::ChartDefault,
        UpdateStrategy::RollingUpdate,
    )
    .to_common_helm_chart()?;

    // Qovery priority class
    let q_priority_class_chart = QoveryPriorityClassChart::new(
        chart_prefix_path,
        HashSet::from_iter(vec![QoveryPriorityClass::HighPriority]), // Cannot use node critical priority class on GKE autopilot
        HelmChartNamespaces::Qovery, // Cannot install anything inside kube-system namespace when it comes to GKE autopilot
    )
    .to_common_helm_chart()?;

    let qovery_engine = CommonChart {
        chart_info: ChartInfo {
            name: "qovery-engine".to_string(),
            action: get_engine_helm_action_from_location(&chart_config_prerequisites.qovery_engine_location),
            path: chart_path("common/charts/qovery-engine"),
            namespace: HelmChartNamespaces::Qovery,
            timeout_in_seconds: 900,
            values: vec![
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: qovery_api.service_version(EngineServiceType::Engine).map_err(|e| {
                        CommandError::new("cannot get engine version".to_string(), Some(e.to_string()), None)
                    })?,
                },
                // metrics
                ChartSetValue {
                    key: "metrics.enabled".to_string(),
                    value: chart_config_prerequisites.ff_metrics_history_enabled.to_string(),
                },
                // autoscaler
                ChartSetValue {
                    key: "autoscaler.enabled".to_string(),
                    value: "true".to_string(),
                },
                // env vars
                ChartSetValue {
                    key: "environmentVariables.ORGANIZATION".to_string(),
                    value: chart_config_prerequisites.cluster_id.clone(), // cluster id should be used here, not org id (to be fixed when reming nats)
                },
                ChartSetValue {
                    key: "environmentVariables.CLOUD_PROVIDER".to_string(),
                    value: chart_config_prerequisites.cloud_provider.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.REGION".to_string(),
                    value: chart_config_prerequisites.region.to_cloud_provider_format().to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.LIB_ROOT_DIR".to_string(),
                    value: "/home/qovery/lib".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.DOCKER_HOST".to_string(),
                    value: "tcp://0.0.0.0:2375".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.GRPC_SERVER".to_string(),
                    value: chart_config_prerequisites.infra_options.qovery_engine_url.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLUSTER_JWT_TOKEN".to_string(),
                    value: chart_config_prerequisites.infra_options.jwt_token.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLUSTER_ID".to_string(),
                    value: chart_config_prerequisites.cluster_long_id.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.ORGANIZATION_ID".to_string(),
                    value: chart_config_prerequisites.organization_long_id.to_string(),
                },
                // builder (look also in values string)
                ChartSetValue {
                    key: "buildContainer.enabled".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "buildContainer.environmentVariables.BUILDER_CPU_ARCHITECTURES".to_string(),
                    value: chart_config_prerequisites
                        .cpu_architectures
                        .iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<String>>()
                        .join(","),
                },
                // engine resources limits
                ChartSetValue {
                    key: "engineResources.limits.cpu".to_string(),
                    value: "1000m".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.requests.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.limits.memory".to_string(),
                    value: "2Gi".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.requests.memory".to_string(),
                    value: "2Gi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    // chart deployment order matters!!!
    let mut level_0: Vec<Box<dyn HelmChart>> = vec![
        // Box::new(prometheus_service_monitor_crd.clone()), // to be fixed: can cause an error if crd is already installed
        Box::new(q_priority_class_chart),
    ];

    let mut level_1: Vec<Box<dyn HelmChart>> = vec![];
    // If IAM settings are set and activated
    if let Some(aws_iam_eks_user_mapper) = aws_iam_eks_user_mapper {
        level_1.push(Box::new(aws_iam_eks_user_mapper));
    }

    let mut level_2: Vec<Box<dyn HelmChart>> = vec![Box::new(q_storage_class), Box::new(aws_ui_view), Box::new(vpa)];

    let mut level_3: Vec<Box<dyn HelmChart>> = vec![];

    let mut level_4: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager)];

    let mut level_5: Vec<Box<dyn HelmChart>> = vec![Box::new(cluster_autoscaler)];

    if let Some(qovery_webhook) = qovery_cert_manager_webhook {
        level_5.push(Box::new(qovery_webhook));
    }

    let mut level_6: Vec<Box<dyn HelmChart>> = vec![
        Box::new(metrics_server),
        Box::new(aws_node_term_handler),
        Box::new(external_dns),
    ];

    if chart_config_prerequisites
        .cluster_advanced_settings
        .aws_eks_enable_alb_controller
    {
        level_6.push(Box::new(aws_load_balancer_controller));
    }

    let level_7: Vec<Box<dyn HelmChart>> = vec![Box::new(nginx_ingress)];

    let mut level_8: Vec<Box<dyn HelmChart>> = vec![
        Box::new(cert_manager_config),
        Box::new(cluster_agent),
        Box::new(qovery_shell_agent),
        Box::new(qovery_engine),
        Box::new(k8s_event_logger),
    ];

    // observability
    if let Some(kube_prometheus_stack_chart) = kube_prometheus_stack {
        level_2.push(Box::new(kube_prometheus_stack_chart));
    }
    if let Some(prometheus_adapter_chart) = prometheus_adapter {
        level_3.push(Box::new(prometheus_adapter_chart));
    }
    if let Some(kube_state_metrics_chart) = kube_state_metrics {
        level_3.push(Box::new(kube_state_metrics_chart));
    }
    if let Some(promtail_chart) = promtail {
        level_2.push(Box::new(promtail_chart));
    }
    if let Some(loki_chart) = loki {
        level_3.push(Box::new(loki_chart));
    }
    if let Some(grafana_chart) = grafana {
        level_3.push(Box::new(grafana_chart))
    }

    // pdb infra
    if chart_config_prerequisites.cluster_advanced_settings.infra_pdb_enabled {
        let pdb_infra = QoveryPdbInfraChart::new(
            chart_prefix_path,
            HelmChartNamespaces::Qovery,
            HelmChartNamespaces::Prometheus,
            HelmChartNamespaces::Logging,
        )
        .to_common_helm_chart()?;

        level_8.push(Box::new(pdb_infra));
    }

    // karpenter
    if chart_config_prerequisites.is_karpenter_enabled {
        level_0.push(Box::new(karpenter_crd));

        level_1.push(Box::new(coredns_config.clone()));
        level_1.push(Box::new(karpenter));

        level_2.push(Box::new(karpenter_configuration));

        if chart_config_prerequisites.ff_metrics_history_enabled {
            level_4.push(Box::new(karpenter_with_monitoring))
        }
    } else {
        level_3.push(Box::new(coredns_config));
    }

    info!("charts configuration preparation finished");
    Ok(vec![
        level_0, level_1, level_2, level_3, level_4, level_5, level_6, level_7, level_8,
    ])
}

pub fn get_qovery_terraform_config(
    qovery_terraform_config_file: &str,
    envs: &[(String, String)],
) -> Result<AwsEksQoveryTerraformConfig, CommandError> {
    let content_file = match File::open(qovery_terraform_config_file) {
        Ok(x) => x,
        Err(e) => {
            return Err(CommandError::new(
                "Can't deploy helm chart as Qovery terraform config file has not been rendered by Terraform. Are you running it in dry run mode?".to_string(),
                Some(e.to_string()),
                Some(envs.to_vec()),
            ));
        }
    };
    let reader = BufReader::new(content_file);
    let qovery_terraform_config: AwsEksQoveryTerraformConfig = match serde_json::from_reader(reader) {
        Ok(config) => config,
        Err(e) => {
            return Err(CommandError::new(
                format!("Error while parsing terraform config file {qovery_terraform_config_file}"),
                Some(e.to_string()),
                Some(envs.to_vec()),
            ));
        }
    };
    Ok(qovery_terraform_config)
}
