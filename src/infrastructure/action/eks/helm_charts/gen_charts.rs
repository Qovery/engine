use crate::helm::{
    get_engine_helm_action_from_location, ChartInfo, ChartSetValue, CommonChart, HelmChart, HelmChartNamespaces,
    PriorityClass, QoveryPriorityClass, UpdateStrategy, VpaContainerPolicy,
};
use crate::infrastructure::helm_charts::coredns_config_chart::CoreDNSConfigChart;
use crate::infrastructure::helm_charts::k8s_event_logger::K8sEventLoggerChart;
use crate::infrastructure::helm_charts::nginx_ingress_chart::NginxIngressChart;
use crate::infrastructure::helm_charts::promtail_chart::PromtailChart;
use crate::infrastructure::helm_charts::qovery_shell_agent_chart::QoveryShellAgentChart;
use crate::infrastructure::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::infrastructure::helm_charts::vertical_pod_autoscaler::VpaChart;
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartResources, HelmChartResourcesConstraintType, HelmChartTimeout,
    HelmChartVpaType, ToCommonHelmChart,
};
use crate::infrastructure::models::cloud_provider::Kind;
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

use crate::errors::CommandError;
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;

use crate::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use crate::environment::models::aws::AwsStorageType;
use crate::environment::models::domain::Domain;
use crate::environment::models::ToCloudProviderFormat;
use crate::infrastructure::action::deploy_helms::mk_customer_chart_override_fn;
use crate::infrastructure::action::eks::helm_charts::aws_alb_controller_chart::AwsLoadBalancerControllerChart;
use crate::infrastructure::action::eks::helm_charts::aws_iam_eks_user_mapper_chart::{
    AwsIamEksUserMapperChart, GroupConfig, GroupConfigMapping, KarpenterConfig, SSOConfig,
};
use crate::infrastructure::action::eks::helm_charts::aws_node_term_handler_chart::AwsNodeTermHandlerChart;
use crate::infrastructure::action::eks::helm_charts::cluster_autoscaler_chart::ClusterAutoscalerChart;
use crate::infrastructure::action::eks::helm_charts::karpenter::KarpenterChart;
use crate::infrastructure::action::eks::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use crate::infrastructure::action::eks::helm_charts::karpenter_crd::KarpenterCrdChart;
use crate::infrastructure::action::eks::helm_charts::EksChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::cert_manager_chart::CertManagerChart;
use crate::infrastructure::helm_charts::cert_manager_config_chart::CertManagerConfigsChart;
use crate::infrastructure::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::infrastructure::helm_charts::grafana_chart::{
    CloudWatchConfig, GrafanaAdminUser, GrafanaChart, GrafanaDatasources,
};
use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::KubePrometheusStackChart;
use crate::infrastructure::helm_charts::kube_state_metrics::KubeStateMetricsChart;
use crate::infrastructure::helm_charts::loki_chart::{
    LokiChart, LokiObjectBucketConfiguration, S3LokiChartConfiguration,
};
use crate::infrastructure::helm_charts::metrics_server_chart::MetricsServerChart;
use crate::infrastructure::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
use crate::infrastructure::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
use crate::infrastructure::helm_charts::qovery_cluster_agent_chart::QoveryClusterAgentChart;
use crate::infrastructure::helm_charts::qovery_priority_class_chart::QoveryPriorityClassChart;
use crate::io_models::QoveryIdentifier;
use chrono::Duration;
use std::collections::HashSet;
use std::iter::FromIterator;
use url::Url;

pub(super) fn eks_helm_charts(
    chart_config_prerequisites: &EksChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    qovery_api: &dyn QoveryApi,
    domain: &Domain,
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
    let get_chart_override_fn =
        mk_customer_chart_override_fn(chart_config_prerequisites.customer_helm_charts_override.clone());

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
        Some(
            chart_config_prerequisites
                .cluster_advanced_settings
                .k8s_storage_class_fast_ssd
                .to_model(),
        ),
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
                chart_config_prerequisites.aws_iam_eks_user_mapper_role_arn.clone(),
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
                        aws_account_id: chart_config_prerequisites.aws_account_id.clone(),
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

    // Vertical pod autoscaler
    let vpa = VpaChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartResourcesConstraintType::ChartDefault,
        true,
        HelmChartNamespaces::KubeSystem,
        false,
    )
    .to_common_helm_chart()?;

    // Karpenter
    let karpenter = KarpenterChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cluster_name.to_string(),
        chart_config_prerequisites.karpenter_controller_aws_role_arn.clone(),
        chart_config_prerequisites.is_karpenter_enabled,
        false,
        chart_config_prerequisites.kubernetes_version_upgrade_requested,
    )
    .to_common_helm_chart()?;

    // Karpenter CRD
    let karpenter_crd = KarpenterCrdChart::new(chart_prefix_path).to_common_helm_chart()?;

    let karpenter_with_monitoring = KarpenterChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cluster_name.to_string(),
        chart_config_prerequisites.karpenter_controller_aws_role_arn.clone(),
        chart_config_prerequisites.is_karpenter_enabled,
        true,
        chart_config_prerequisites.kubernetes_version_upgrade_requested,
    )
    .to_common_helm_chart()?;

    // Karpenter Configuration
    let karpenter_configuration = KarpenterConfigurationChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cluster_name.to_string(),
        chart_config_prerequisites.is_karpenter_enabled,
        chart_config_prerequisites.cluster_security_group_id.clone(),
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
        chart_config_prerequisites.aws_iam_cluster_autoscaler_role_arn.clone(),
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
        chart_config_prerequisites.aws_iam_alb_controller_arn.clone(),
        chart_config_prerequisites.cluster_name.clone(),
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartVpaType::EnabledWithConstraints(VpaContainerPolicy::new(
            "*".to_string(),
            Some(KubernetesCpuResourceUnit::MilliCpu(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .aws_eks_alb_controller_vpa_min_vcpu_in_milli_cpu,
            )),
            Some(KubernetesCpuResourceUnit::MilliCpu(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .aws_eks_alb_controller_vpa_max_vcpu_in_milli_cpu,
            )),
            Some(KubernetesMemoryResourceUnit::MebiByte(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .aws_eks_alb_controller_vpa_min_memory_in_mib,
            )),
            Some(KubernetesMemoryResourceUnit::MebiByte(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .aws_eks_alb_controller_vpa_max_memory_in_mib,
            )),
        )),
        chart_config_prerequisites.alb_controller_already_deployed
            && chart_config_prerequisites
                .cluster_advanced_settings
                .aws_eks_enable_alb_controller,
    )
    .to_common_helm_chart()?;

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
                chart_config_prerequisites.is_karpenter_enabled,
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
                    bucketname: Some(chart_config_prerequisites.aws_s3_loki_bucket_name.clone()),
                    s3_config: Some(chart_config_prerequisites.loki_storage_config_aws_s3.clone()),
                    aws_iam_loki_role_arn: Some(chart_config_prerequisites.aws_iam_loki_role_arn.clone()),
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
                chart_config_prerequisites.is_karpenter_enabled,
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
                chart_config_prerequisites.is_karpenter_enabled,
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
                        chart_config_prerequisites.aws_iam_cloudwatch_role_arn.clone(),
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
        chart_config_prerequisites.organization_long_id.to_string(),
        chart_config_prerequisites.organization_id.clone(),
        chart_config_prerequisites.cluster_long_id.to_string(),
        chart_config_prerequisites.cluster_id.clone(),
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
            .nginx_controller_use_forwarded_headers,
        chart_config_prerequisites
            .cluster_advanced_settings
            .nginx_controller_compute_full_forwarded_for,
        chart_config_prerequisites
            .cluster_advanced_settings
            .nginx_controller_log_format_escaping
            .to_model(),
        chart_config_prerequisites
            .cluster_advanced_settings
            .aws_eks_enable_alb_controller,
        chart_config_prerequisites
            .cluster_advanced_settings
            .nginx_controller_http_snippet
            .as_ref()
            .map(|nginx_controller_http_snippet_io| nginx_controller_http_snippet_io.to_model()),
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
        HashSet::from_iter(vec![QoveryPriorityClass::StandardPriority, QoveryPriorityClass::HighPriority]), // Cannot use node critical priority class on GKE autopilot
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

    let mut level_2: Vec<Box<dyn HelmChart>> = vec![];

    let level_3: Vec<Box<dyn HelmChart>> = vec![
        // This chart is required in order to install CRDs and declare later charts with VPA
        // It will be installed only if chart doesn't exist already on the cluster in order to avoid
        // disabling VPA on VPA controller at each update
        Box::new(
            VpaChart::new(
                chart_prefix_path,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartResourcesConstraintType::ChartDefault,
                false, // <- VPA not activated
                HelmChartNamespaces::KubeSystem,
                true, // <- wont be deployed if already exists
            )
            .to_common_helm_chart()?,
        ),
    ];

    let mut level_4: Vec<Box<dyn HelmChart>> = vec![Box::new(q_storage_class), Box::new(vpa)];

    let mut level_5: Vec<Box<dyn HelmChart>> = vec![];

    let mut level_6: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager)];

    let mut level_7: Vec<Box<dyn HelmChart>> = vec![Box::new(cluster_autoscaler)];

    if let Some(qovery_webhook) = qovery_cert_manager_webhook {
        level_7.push(Box::new(qovery_webhook));
    }

    let mut level_8: Vec<Box<dyn HelmChart>> = vec![
        Box::new(metrics_server),
        Box::new(aws_node_term_handler),
        Box::new(external_dns),
    ];

    if chart_config_prerequisites
        .cluster_advanced_settings
        .aws_eks_enable_alb_controller
        || chart_config_prerequisites.alb_controller_already_deployed
    {
        level_8.push(Box::new(aws_load_balancer_controller));
    }

    let level_9: Vec<Box<dyn HelmChart>> = vec![Box::new(nginx_ingress)];

    let level_10: Vec<Box<dyn HelmChart>> = vec![
        Box::new(cert_manager_config),
        Box::new(cluster_agent),
        Box::new(qovery_shell_agent),
        Box::new(qovery_engine),
        Box::new(k8s_event_logger),
    ];

    // observability
    if let Some(kube_prometheus_stack_chart) = kube_prometheus_stack {
        level_4.push(Box::new(kube_prometheus_stack_chart));
    }
    if let Some(prometheus_adapter_chart) = prometheus_adapter {
        level_5.push(Box::new(prometheus_adapter_chart));
    }
    if let Some(kube_state_metrics_chart) = kube_state_metrics {
        level_5.push(Box::new(kube_state_metrics_chart));
    }
    if let Some(promtail_chart) = promtail {
        level_4.push(Box::new(promtail_chart));
    }
    if let Some(loki_chart) = loki {
        level_5.push(Box::new(loki_chart));
    }
    if let Some(grafana_chart) = grafana {
        level_5.push(Box::new(grafana_chart))
    }

    // karpenter
    if chart_config_prerequisites.is_karpenter_enabled {
        level_0.push(Box::new(karpenter_crd));

        level_1.push(Box::new(coredns_config.clone()));
        level_1.push(Box::new(karpenter));

        level_2.push(Box::new(karpenter_configuration));

        if chart_config_prerequisites.ff_metrics_history_enabled {
            level_6.push(Box::new(karpenter_with_monitoring))
        }
    } else {
        level_5.push(Box::new(coredns_config));
    }

    info!("charts configuration preparation finished");
    Ok(vec![
        level_0, level_1, level_2, // <- after this point, pods can be created outside of fargate
        level_3, // <- after this point, VPA can be activated on pods
        level_4, level_5, level_6, level_7, level_8, level_9, level_10,
    ])
}
