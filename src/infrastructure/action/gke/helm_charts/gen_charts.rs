use super::GkeChartsConfigPrerequisites;
use crate::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use crate::environment::models::domain::Domain;
use crate::errors::CommandError;
use crate::helm::{HelmChart, HelmChartNamespaces, PriorityClass, QoveryPriorityClass, UpdateStrategy};
use crate::infrastructure::action::deploy_helms::mk_customer_chart_override_fn;
use crate::infrastructure::action::gen_metrics_charts::{CloudProviderMetricsConfig, generate_metrics_config};
use crate::infrastructure::helm_charts::cert_manager_chart::CertManagerChart;
use crate::infrastructure::helm_charts::cert_manager_config_chart::CertManagerConfigsChart;
use crate::infrastructure::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::infrastructure::helm_charts::k8s_event_logger::K8sEventLoggerChart;
use crate::infrastructure::helm_charts::loki_chart::{
    GCSLokiChartConfiguration, LokiChart, LokiObjectBucketConfiguration,
};
use crate::infrastructure::helm_charts::nginx_ingress_chart::{NginxIngressChart, NginxOptions};
use crate::infrastructure::helm_charts::promtail_chart::PromtailChart;
use crate::infrastructure::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
use crate::infrastructure::helm_charts::qovery_cluster_agent_chart::QoveryClusterAgentChart;
use crate::infrastructure::helm_charts::qovery_priority_class_chart::QoveryPriorityClassChart;
use crate::infrastructure::helm_charts::qovery_shell_agent_chart::QoveryShellAgentChart;
use crate::infrastructure::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartResources, HelmChartResourcesConstraintType, HelmChartTimeout,
    ToCommonHelmChart,
};
use crate::infrastructure::models::cloud_provider::Kind;
use crate::infrastructure::models::cloud_provider::Kind as CloudProviderKind;
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
use crate::io_models::QoveryIdentifier;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use std::collections::HashSet;
use time::Duration;
use url::Url;

pub(super) fn gke_helm_charts(
    chart_config_prerequisites: &GkeChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    qovery_api: &dyn QoveryApi,
    domain: &Domain,
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
    let get_chart_override_fn =
        mk_customer_chart_override_fn(chart_config_prerequisites.customer_helm_charts_override.clone());

    let prometheus_namespace = HelmChartNamespaces::Qovery;
    let prometheus_internal_url = format!("http://prometheus-operated.{prometheus_namespace}.svc");
    let loki_namespace = HelmChartNamespaces::Qovery;
    let loki_kube_dns_name = format!("loki.{loki_namespace}.svc:3100");

    let metrics_config = generate_metrics_config(
        CloudProviderMetricsConfig::Gke(chart_config_prerequisites),
        chart_prefix_path,
        &prometheus_internal_url,
        prometheus_namespace,
        get_chart_override_fn.clone(),
        chart_config_prerequisites.cluster_long_id.to_string().as_str(),
    )?;

    // Qovery storage class
    let q_storage_class_chart = QoveryStorageClassChart::new(
        chart_prefix_path,
        CloudProviderKind::Gcp,
        HashSet::from_iter(vec![QoveryStorageType::Ssd, QoveryStorageType::Hdd]), // TODO(ENG-1800): Add Cold and Nvme
        HelmChartNamespaces::Qovery, // Cannot install anything inside kube-system namespace when it comes to GKE autopilot
        Some(
            chart_config_prerequisites
                .cluster_advanced_settings
                .k8s_storage_class_fast_ssd
                .to_model(),
        ),
    )
    .to_common_helm_chart()?;

    // Qovery priority class
    let q_priority_class_chart = QoveryPriorityClassChart::new(
        chart_prefix_path,
        HashSet::from_iter(vec![QoveryPriorityClass::StandardPriority, QoveryPriorityClass::HighPriority]), // Cannot use node critical priority class on GKE autopilot
        HelmChartNamespaces::Qovery, // Cannot install anything inside kube-system namespace when it comes to GKE autopilot
    )
    .to_common_helm_chart()?;

    // External DNS
    let external_dns_chart = ExternalDNSChart::new(
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

    // Metrics server is built-in GCP cluster, no need to manage it
    // VPA is built-in GCP cluster, no need to manage it
    let loki: Option<Box<dyn HelmChart>> = match chart_config_prerequisites.ff_log_history_enabled {
        false => None,
        true => Some(Box::new(
            LokiChart::new(
                chart_prefix_path,
                loki_namespace,
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .loki_log_retention_in_week,
                LokiObjectBucketConfiguration::GCS(GCSLokiChartConfiguration {
                    gcp_service_account: Some(
                        chart_config_prerequisites
                            .loki_logging_service_account_email
                            .to_string(),
                    ),
                    bucketname: Some(chart_config_prerequisites.logs_bucket_name.to_string()),
                }),
                get_chart_override_fn.clone(),
                true,
                Some(500), // GCP need at least 500m for pod with antiAffinity
                HelmChartResourcesConstraintType::Constrained(HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(500), // {"[denied by autogke-pod-limit-constraints]":["workload 'loki-0' cpu requests '250m' is lower than the Autopilot minimum required of '500m' for using pod anti affinity."]}
                    request_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000), // {"[denied by autogke-pod-limit-constraints]":["workload 'loki-0' cpu requests '250m' is lower than the Autopilot minimum required of '500m' for using pod anti affinity."]}
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(2),
                }),
                HelmChartTimeout::Custom(Duration::seconds(1200)), // GCP might have a lag in role / authorizations to be working in case you just assigned them, so just allow Loki to wait a bit before failing
                false,
            )
            .to_common_helm_chart()?,
        )),
    };

    let promtail: Option<Box<dyn HelmChart>> = match chart_config_prerequisites.ff_log_history_enabled {
        false => None,
        true => Some(Box::new(
            PromtailChart::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder, // use GCP override
                loki_kube_dns_name,
                get_chart_override_fn.clone(),
                true,
                HelmChartNamespaces::Qovery,
                PriorityClass::Qovery(QoveryPriorityClass::HighPriority),
                false,
                chart_config_prerequisites.metrics_parameters.is_some(),
            )
            .to_common_helm_chart()?,
        )),
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
        HelmChartNamespaces::Qovery,
        HelmChartNamespaces::Qovery, // Leader election defaults to kube-system which is not permitted on GKE autopilot
    )
    .to_common_helm_chart()?;

    // Cert Manager Configs
    let cert_manager_config = CertManagerConfigsChart::new(
        chart_prefix_path,
        &chart_config_prerequisites.lets_encrypt_config,
        &chart_config_prerequisites.dns_provider_config,
        chart_config_prerequisites.managed_dns_helm_format.to_string(),
        HelmChartNamespaces::Qovery,
    )
    .to_common_helm_chart()?;

    // Cert Manager Webhook
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
        Kind::Gcp,
        chart_config_prerequisites.organization_long_id.to_string(),
        chart_config_prerequisites.organization_id.clone(),
        chart_config_prerequisites.cluster_long_id.to_string(),
        chart_config_prerequisites.cluster_id.clone(),
        KubernetesKind::Gke,
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
            is_alb_enabled: false, // only for AWS
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
            true => {
                match loki {
                    Some(_) => Some(Url::parse("http://loki.qovery.svc.cluster.local:3100").map_err(|e| {
                        CommandError::new("cannot parse Loki url".to_string(), Some(e.to_string()), None)
                    })?),
                    None => None,
                }
            }
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

    let prometheus_operator_crds_chart = metrics_config
        .prometheus_operator_crds_chart
        .map(|chart| Box::new(chart) as Box<dyn HelmChart>);

    let kube_prometheus_stack_chart = metrics_config
        .kube_prometheus_stack_chart
        .map(|chart| Box::new(chart) as Box<dyn HelmChart>);

    let thanos_chart = metrics_config
        .thanos_chart
        .map(|chart| Box::new(chart) as Box<dyn HelmChart>);

    // chart deployment order matters!!!
    // Helm chart deployment order

    // Add prometheus CRDs early to avoid issues with other charts
    let level_0: Vec<Option<Box<dyn HelmChart>>> = vec![prometheus_operator_crds_chart];
    let level_1: Vec<Option<Box<dyn HelmChart>>> = vec![
        Some(Box::new(q_storage_class_chart)),
        Some(Box::new(q_priority_class_chart)),
        kube_prometheus_stack_chart,
        promtail,
    ];
    let level_2: Vec<Option<Box<dyn HelmChart>>> = vec![loki, thanos_chart];
    let level_3: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(cert_manager))];
    let level_4: Vec<Option<Box<dyn HelmChart>>> = vec![qovery_cert_manager_webhook];
    let level_5: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(external_dns_chart))];
    let level_6: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(nginx_ingress))];
    let level_7: Vec<Option<Box<dyn HelmChart>>> = vec![
        Some(Box::new(cert_manager_config)),
        Some(Box::new(qovery_cluster_agent)),
        Some(Box::new(qovery_shell_agent)),
        Some(Box::new(k8s_event_logger)),
    ];

    Ok(vec![
        level_0.into_iter().flatten().collect(),
        level_1.into_iter().flatten().collect(),
        level_2.into_iter().flatten().collect(),
        level_3.into_iter().flatten().collect(),
        level_4.into_iter().flatten().collect(),
        level_5.into_iter().flatten().collect(),
        level_6.into_iter().flatten().collect(),
        level_7.into_iter().flatten().collect(),
    ])
}
