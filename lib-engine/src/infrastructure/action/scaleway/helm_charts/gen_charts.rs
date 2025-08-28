use crate::helm::{
    ChartInfo, ChartSetValue, CommonChart, HelmChart, HelmChartNamespaces, PriorityClass, QoveryPriorityClass,
    UpdateStrategy, get_engine_helm_action_from_location,
};
use crate::infrastructure::helm_charts::k8s_event_logger::K8sEventLoggerChart;
use crate::infrastructure::helm_charts::nginx_ingress_chart::{NginxIngressChart, NginxOptions};
use crate::infrastructure::helm_charts::promtail_chart::PromtailChart;
use crate::infrastructure::helm_charts::qovery_shell_agent_chart::QoveryShellAgentChart;
use crate::infrastructure::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::infrastructure::helm_charts::vertical_pod_autoscaler::VpaChart;
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartResources, HelmChartResourcesConstraintType, HelmChartTimeout,
    ToCommonHelmChart,
};
use crate::infrastructure::models::cloud_provider::Kind;
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

use crate::errors::CommandError;
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;

use super::KapsuleChartsConfigPrerequisites;
use crate::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use crate::environment::models::domain::Domain;
use crate::infrastructure::action::deploy_helms::mk_customer_chart_override_fn;
use crate::infrastructure::helm_charts::cert_manager_chart::CertManagerChart;
use crate::infrastructure::helm_charts::cert_manager_config_chart::CertManagerConfigsChart;
use crate::infrastructure::helm_charts::coredns_config_chart::CoreDNSConfigChart;
use crate::infrastructure::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::infrastructure::helm_charts::grafana_chart::{GrafanaAdminUser, GrafanaChart, GrafanaDatasources};
use crate::infrastructure::helm_charts::loki_chart::{
    LokiChart, LokiObjectBucketConfiguration, S3LokiChartConfiguration,
};
use crate::infrastructure::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
use crate::infrastructure::helm_charts::qovery_cluster_agent_chart::QoveryClusterAgentChart;
use crate::infrastructure::helm_charts::qovery_priority_class_chart::QoveryPriorityClassChart;
use crate::io_models::QoveryIdentifier;
// use crate::io_models::metrics::MetricsConfiguration;
use crate::infrastructure::action::gen_metrics_charts::{CloudProviderMetricsConfig, generate_metrics_config};
use std::collections::HashSet;
use std::iter::FromIterator;
use url::Url;

pub fn kapsule_helm_charts(
    chart_config_prerequisites: &KapsuleChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    qovery_api: &dyn QoveryApi,
    domain: &Domain,
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
    info!("preparing chart configuration to be deployed");
    let kind_provider = Kind::Scw;
    let get_chart_override_fn =
        mk_customer_chart_override_fn(chart_config_prerequisites.customer_helm_charts_override.clone());

    let chart_prefix = chart_prefix_path.unwrap_or("./");
    let chart_path = |x: &str| -> String { format!("{}/{}", &chart_prefix, x) };
    let prometheus_namespace = HelmChartNamespaces::Prometheus;
    let prometheus_internal_url = format!("http://prometheus-operated.{prometheus_namespace}.svc");
    let loki_namespace = HelmChartNamespaces::Logging;
    let loki_kube_dns_name = format!("loki.{loki_namespace}.svc:3100");

    let metrics_config = generate_metrics_config(
        CloudProviderMetricsConfig::Kapsule(chart_config_prerequisites),
        chart_prefix_path,
        &prometheus_internal_url,
        prometheus_namespace,
        get_chart_override_fn.clone(),
        chart_config_prerequisites.cluster_long_id.to_string().as_str(),
    )?;

    let prometheus_operator_crds_chart = metrics_config
        .prometheus_operator_crds_chart
        .map(|chart| Box::new(chart) as Box<dyn HelmChart>);

    let kube_prometheus_stack_chart = metrics_config
        .kube_prometheus_stack_chart
        .map(|chart| Box::new(chart) as Box<dyn HelmChart>);

    let thanos_chart = metrics_config
        .thanos_chart
        .map(|chart| Box::new(chart) as Box<dyn HelmChart>);

    // Qovery storage class
    let q_storage_class = QoveryStorageClassChart::new(
        chart_prefix_path,
        kind_provider,
        HashSet::from_iter(vec![QoveryStorageType::Ssd]),
        HelmChartNamespaces::KubeSystem,
        Some(
            chart_config_prerequisites
                .cluster_advanced_settings
                .k8s_storage_class_fast_ssd
                .to_model(),
        ),
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
        chart_config_prerequisites
            .cluster_advanced_settings
            .dns_coredns_extra_config
            .clone(),
        HelmChartNamespaces::KubeSystem,
    );

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
                false,
                chart_config_prerequisites.metrics_parameters.is_some(),
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
                // LokiEncryptionType::None, // Scaleway does not support encryption yet.
                loki_namespace.clone(),
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .loki_log_retention_in_week,
                LokiObjectBucketConfiguration::S3(S3LokiChartConfiguration {
                    s3_config: Some(chart_config_prerequisites.loki_storage_config_scaleway_s3.clone()),
                    region: Some(chart_config_prerequisites.zone.region().to_string()),
                    aws_iam_loki_role_arn: None,
                    bucketname: None,
                    insecure: false,
                    use_path_style: true,
                }),
                get_chart_override_fn.clone(),
                true,
                None,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartTimeout::ChartDefault,
                false,
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
                    cloudwatch_config: None,
                },
                "scw-sbv-ssd-0".to_string(), // TODO(benjaminch): introduce proper type here
            )
            .to_common_helm_chart()?,
        ),
    };

    // Cert Manager chart
    let cert_manager = CertManagerChart::new(
        chart_prefix_path,
        chart_config_prerequisites.metrics_parameters.is_some(),
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
        chart_config_prerequisites.metrics_parameters.is_some(),
        get_chart_override_fn.clone(),
        domain.clone(),
        Kind::Scw,
        chart_config_prerequisites.organization_long_id.to_string(),
        chart_config_prerequisites.organization_id.clone(),
        chart_config_prerequisites.cluster_long_id.to_string(),
        chart_config_prerequisites.cluster_id.clone(),
        KubernetesKind::ScwKapsule,
        chart_config_prerequisites.cluster_creation_date,
        NginxOptions {
            nginx_hpa_minimum_replicas: Some(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .nginx_hpa_min_number_instances,
            ),
            nginx_hpa_maximum_replicas: Some(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .nginx_hpa_max_number_instances,
            ),
            nginx_hpa_target_cpu_utilization_percentage: Some(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .nginx_hpa_cpu_utilization_percentage_threshold,
            ),
            namespace: HelmChartNamespaces::NginxIngress,
            loadbalancer_size: Some(
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .load_balancer_size
                    .clone(),
            ),
            enable_real_ip: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_controller_enable_client_ip,
            use_forwarded_headers: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_controller_use_forwarded_headers,
            compute_full_forwarded_for: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_controller_compute_full_forwarded_for,
            log_format_escaping: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_controller_log_format_escaping
                .to_model(),
            is_alb_enabled: false, // AWS only
            http_snippet: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_controller_http_snippet
                .as_ref()
                .map(|nginx_controller_http_snippet_io| nginx_controller_http_snippet_io.to_model()),
            server_snippet: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_controller_server_snippet
                .as_ref()
                .map(|nginx_controller_server_snippet_io| nginx_controller_server_snippet_io.to_model()),
            limit_request_status_code: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_controller_limit_request_status_code
                .as_ref()
                .map(|v| v.to_model().map_err(CommandError::from))
                .transpose()?,
            nginx_controller_custom_http_errors: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_controller_custom_http_errors
                .clone(),
            nginx_default_backend_enabled: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_default_backend_enabled,
            nginx_default_backend_image_repository: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_default_backend_image_repository
                .clone(),
            nginx_default_backend_image_tag: chart_config_prerequisites
                .cluster_advanced_settings
                .nginx_default_backend_image_tag
                .clone(),
            default_ssl_certificate: None,
            publish_status_address: None,
            replica_count: None,
            metal_lb_load_balancer_ip: None,
            external_dns_target: None,
        },
    )
    .to_common_helm_chart()?;

    // K8s Event Logger
    let k8s_event_logger = K8sEventLoggerChart::new(
        chart_prefix_path,
        true,
        HelmChartNamespaces::Qovery,
        chart_config_prerequisites.metrics_parameters.is_some(),
    )
    .to_common_helm_chart()?;

    // Qovery cluster agent
    let qovery_cluster_agent = QoveryClusterAgentChart::new(
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
        &chart_config_prerequisites.infra_options.jwt_token,
        QoveryIdentifier::new(chart_config_prerequisites.cluster_long_id),
        QoveryIdentifier::new(chart_config_prerequisites.organization_long_id),
        HelmChartResourcesConstraintType::ChartDefault,
        UpdateStrategy::RollingUpdate,
        true,
        false,
        metrics_config.metrics_query_url,
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
                ChartSetValue {
                    key: "metrics.enabled".to_string(),
                    value: chart_config_prerequisites.metrics_parameters.is_some().to_string(),
                },
                // autoscaler
                ChartSetValue {
                    key: "autoscaler.enabled".to_string(),
                    value: "true".to_string(),
                },
                // env vars
                ChartSetValue {
                    key: "environmentVariables.ORGANIZATION".to_string(),
                    value: chart_config_prerequisites.organization_id.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLOUD_PROVIDER".to_string(),
                    value: "scw".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.REGION".to_string(),
                    value: chart_config_prerequisites.zone.to_string(),
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
                    value: "AMD64".to_string(), // Scaleway doesn't support ARM arch yet
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

    let mut level_1: Vec<Box<dyn HelmChart>> = vec![
        Box::new(q_storage_class),
        Box::new(coredns_config),
        Box::new(vpa),
        Box::new(q_priority_class_chart),
    ];

    let mut level_2: Vec<Box<dyn HelmChart>> = vec![];

    let level_3: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager)];

    let level_4: Vec<Box<dyn HelmChart>> = if let Some(qovery_webhook) = qovery_cert_manager_webhook {
        vec![Box::new(qovery_webhook)]
    } else {
        vec![]
    };

    let level_5: Vec<Box<dyn HelmChart>> = vec![Box::new(external_dns)];

    let level_6: Vec<Box<dyn HelmChart>> = vec![Box::new(nginx_ingress)];

    let level_7: Vec<Box<dyn HelmChart>> = vec![
        Box::new(cert_manager_config),
        Box::new(qovery_cluster_agent),
        Box::new(qovery_shell_agent),
        Box::new(qovery_engine),
        Box::new(k8s_event_logger),
    ];

    // observability
    if let Some(prometheus_operator_crds_chart) = prometheus_operator_crds_chart {
        level_0.push(prometheus_operator_crds_chart)
    }
    if let Some(kube_prometheus_stack_chart) = kube_prometheus_stack_chart {
        level_1.push(kube_prometheus_stack_chart)
    }
    if let Some(promtail_chart) = promtail {
        level_1.push(Box::new(promtail_chart));
    }
    if let Some(thanos_chart) = thanos_chart {
        level_2.push(thanos_chart)
    }
    if let Some(loki_chart) = loki {
        level_2.push(Box::new(loki_chart));
    }
    if let Some(grafana_chart) = grafana {
        level_2.push(Box::new(grafana_chart))
    }

    info!("charts configuration preparation finished");
    Ok(vec![level_0, level_1, level_2, level_3, level_4, level_5, level_6, level_7])
}
