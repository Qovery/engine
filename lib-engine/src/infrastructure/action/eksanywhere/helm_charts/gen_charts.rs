use crate::helm::{HelmChart, HelmChartNamespaces, PriorityClass, QoveryPriorityClass, UpdateStrategy};
use crate::infrastructure::helm_charts::k8s_event_logger::K8sEventLoggerChart;
use crate::infrastructure::helm_charts::nginx_ingress_chart::{NginxIngressChart, NginxOptions};
use crate::infrastructure::helm_charts::promtail_chart::PromtailChart;
use crate::infrastructure::helm_charts::qovery_shell_agent_chart::QoveryShellAgentChart;
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

use crate::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use crate::environment::models::domain::Domain;
use crate::infrastructure::action::deploy_helms::mk_customer_chart_override_fn;
use crate::infrastructure::action::eksanywhere::helm_charts::EksAnywhereChartsConfigPrerequisites;
use crate::infrastructure::action::eksanywhere::helm_charts::metal_lb_chart::MetalLbChart;
use crate::infrastructure::action::eksanywhere::helm_charts::metal_lb_config_chart::MetalLbConfigChart;
use crate::infrastructure::helm_charts::cert_manager_config_chart::CertManagerConfigsChart;
use crate::infrastructure::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::infrastructure::helm_charts::loki_chart::{LokiChart, LokiObjectBucketConfiguration};
use crate::infrastructure::helm_charts::metrics_server_chart::MetricsServerChart;
use crate::infrastructure::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
use crate::infrastructure::helm_charts::qovery_cluster_agent_chart::QoveryClusterAgentChart;
use crate::infrastructure::helm_charts::qovery_priority_class_chart::QoveryPriorityClassChart;
use crate::io_models::QoveryIdentifier;
use std::collections::HashSet;
use std::iter::FromIterator;
use url::Url;

pub(super) fn eks_anywhere_helm_charts(
    chart_config_prerequisites: &EksAnywhereChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    qovery_api: &dyn QoveryApi,
    domain: &Domain,
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
    let get_chart_override_fn =
        mk_customer_chart_override_fn(chart_config_prerequisites.customer_helm_charts_override.clone());

    // VPA
    let vpa = VpaChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartResourcesConstraintType::ChartDefault,
        true,
        HelmChartNamespaces::Qovery,
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
        HelmChartNamespaces::Qovery,
    )
    .to_common_helm_chart()?;

    // Promtail & Loki
    let loki_namespace = HelmChartNamespaces::Qovery;
    let loki_kube_dns_name = format!("loki.{loki_namespace}.svc:3100");

    let promtail = match chart_config_prerequisites.ff_log_history_enabled {
        false => None,
        true => Some(
            PromtailChart::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                loki_kube_dns_name,
                get_chart_override_fn.clone(),
                true,
                HelmChartNamespaces::Qovery,
                PriorityClass::Default,
                false,
                false,
            )
            .to_common_helm_chart()?,
        ),
    };

    let loki = match chart_config_prerequisites.ff_log_history_enabled {
        false => None,
        true => Some(
            LokiChart::new(
                chart_prefix_path,
                loki_namespace,
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .loki_log_retention_in_week,
                LokiObjectBucketConfiguration::Local,
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

    // K8s Event Logger
    let k8s_event_logger =
        K8sEventLoggerChart::new(chart_prefix_path, true, HelmChartNamespaces::Qovery, false).to_common_helm_chart()?;

    // Metrics server
    let metrics_server = MetricsServerChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::ChartDefault,
        HelmChartNamespaces::Qovery,
        UpdateStrategy::RollingUpdate,
        true,
        true, // needs to support specific param (args: - --kubelet-insecure-tls)
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
        chart_config_prerequisites.metrics_parameters.is_some(),
        get_chart_override_fn.clone(),
        domain.clone(),
        Kind::OnPremise,
        chart_config_prerequisites.organization_long_id.to_string(),
        chart_config_prerequisites.organization_id.clone(),
        chart_config_prerequisites.cluster_long_id.to_string(),
        chart_config_prerequisites.cluster_id.clone(),
        KubernetesKind::EksAnywhere,
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
            namespace: HelmChartNamespaces::Qovery,
            loadbalancer_size: None,
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
            is_alb_enabled: chart_config_prerequisites
                .cluster_advanced_settings
                .aws_eks_enable_alb_controller,
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
            default_ssl_certificate: Some(
                chart_config_prerequisites
                    .infra_options
                    .infrastructure_charts_parameters
                    .nginx_chart_overrides
                    .default_ssl_certificate
                    .clone(),
            ),
            publish_status_address: Some(
                chart_config_prerequisites
                    .infra_options
                    .infrastructure_charts_parameters
                    .nginx_chart_overrides
                    .publish_status_address
                    .clone(),
            ),
            replica_count: Some(
                chart_config_prerequisites
                    .infra_options
                    .infrastructure_charts_parameters
                    .nginx_chart_overrides
                    .replica_count,
            ),
            metal_lb_load_balancer_ip: Some(
                chart_config_prerequisites
                    .infra_options
                    .infrastructure_charts_parameters
                    .nginx_chart_overrides
                    .annotation_metal_lb_load_balancer_ips
                    .clone(),
            ),
            external_dns_target: Some(
                chart_config_prerequisites
                    .infra_options
                    .infrastructure_charts_parameters
                    .nginx_chart_overrides
                    .annotation_external_dns_kubernetes_target
                    .clone(),
            ),
        },
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
        false,
        None,
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

    // Metal lb chart
    let metal_lb = MetalLbChart::new(chart_prefix_path, HelmChartNamespaces::Qovery).to_common_helm_chart()?;
    let metal_lb_config = MetalLbConfigChart::new(
        chart_prefix_path,
        HelmChartNamespaces::Qovery,
        chart_config_prerequisites
            .infra_options
            .infrastructure_charts_parameters
            .metal_lb_chart_overrides
            .ip_address_pools
            .clone(),
    )
    .to_common_helm_chart()?;

    // Cert-manager
    // EKS anywhere has its own cert-manager installed so it shouldn't be installed by Qovery
    // However, we should install cert-manager webhook and cert-manager configs
    let mut qovery_cert_manager_webhook: Option<Box<dyn HelmChart>> = None;
    if let DnsProviderConfiguration::QoveryDns(qovery_dns_config) = &chart_config_prerequisites.dns_provider_config {
        qovery_cert_manager_webhook = Some(Box::new(
            QoveryCertManagerWebhookChart::new(
                chart_prefix_path,
                qovery_dns_config.clone(),
                HelmChartResourcesConstraintType::ChartDefault,
                UpdateStrategy::RollingUpdate,
                HelmChartNamespaces::Qovery,
                HelmChartNamespaces::Qovery,
            )
            .to_common_helm_chart()?,
        ));
    }

    // Cert Manager Configs
    let cert_manager_config: Option<Box<dyn HelmChart>> = Some(Box::new(
        CertManagerConfigsChart::new(
            chart_prefix_path,
            &chart_config_prerequisites.lets_encrypt_config,
            &chart_config_prerequisites.dns_provider_config,
            chart_config_prerequisites.managed_dns_helm_format.to_string(),
            HelmChartNamespaces::Qovery,
        )
        .to_common_helm_chart()?,
    ));

    // Set deploying order
    let level_0: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(q_priority_class_chart))];

    let level_1: Vec<Option<Box<dyn HelmChart>>> = vec![
        // This chart is required in order to install CRDs and declare later charts with VPA
        // It will be installed only if chart doesn't exist already on the cluster in order to avoid
        // disabling VPA on VPA controller at each update
        Some(Box::new(
            VpaChart::new(
                chart_prefix_path,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartResourcesConstraintType::ChartDefault,
                false, // <- VPA not activated
                HelmChartNamespaces::Qovery,
                true, // <- wont be deployed if already exists
            )
            .to_common_helm_chart()?,
        )),
    ];

    let level_2: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(vpa))];

    let mut level_3: Vec<Option<Box<dyn HelmChart>>> = vec![];
    if let Some(promtail_chart) = promtail {
        level_3.push(Some(Box::new(promtail_chart)));
    }
    if let Some(loki_chart) = loki {
        level_3.push(Some(Box::new(loki_chart)));
    }

    let level_4: Vec<Option<Box<dyn HelmChart>>> = vec![];
    let level_5: Vec<Option<Box<dyn HelmChart>>> = vec![qovery_cert_manager_webhook];

    let level_6: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(metrics_server)), Some(Box::new(external_dns))];
    let level_7: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(metal_lb))];
    let level_8: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(metal_lb_config)), Some(Box::new(nginx_ingress))];

    let level_9: Vec<Option<Box<dyn HelmChart>>> = vec![];

    let level_10: Vec<Option<Box<dyn HelmChart>>> = vec![
        Some(Box::new(cluster_agent)),
        Some(Box::new(qovery_shell_agent)),
        Some(Box::new(k8s_event_logger)),
        cert_manager_config,
    ];

    info!("charts configuration preparation finished");
    Ok(vec![
        level_0.into_iter().flatten().collect(),
        level_1.into_iter().flatten().collect(),
        level_2.into_iter().flatten().collect(),
        level_3.into_iter().flatten().collect(),
        level_4.into_iter().flatten().collect(),
        level_5.into_iter().flatten().collect(),
        level_6.into_iter().flatten().collect(),
        level_7.into_iter().flatten().collect(),
        level_8.into_iter().flatten().collect(),
        level_9.into_iter().flatten().collect(),
        level_10.into_iter().flatten().collect(),
    ])
}
