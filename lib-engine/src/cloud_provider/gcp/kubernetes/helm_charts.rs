use crate::cloud_provider::gcp::kubernetes::GkeOptions;
use crate::cloud_provider::gcp::locations::GcpRegion;
use crate::cloud_provider::helm::{HelmChart, HelmChartNamespaces, PriorityClass, QoveryPriorityClass, UpdateStrategy};
use crate::cloud_provider::helm_charts::cert_manager_chart::CertManagerChart;
use crate::cloud_provider::helm_charts::cert_manager_config_chart::CertManagerConfigsChart;
use crate::cloud_provider::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::cloud_provider::helm_charts::k8s_event_logger::K8sEventLoggerChart;
use crate::cloud_provider::helm_charts::kube_prometheus_stack_chart::KubePrometheusStackChart;
use crate::cloud_provider::helm_charts::kube_state_metrics::KubeStateMetricsChart;
use crate::cloud_provider::helm_charts::loki_chart::{
    GCSLokiChartConfiguration, LokiChart, LokiObjectBucketConfiguration,
};
use crate::cloud_provider::helm_charts::nginx_ingress_chart::NginxIngressChart;
use crate::cloud_provider::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
use crate::cloud_provider::helm_charts::promtail_chart::PromtailChart;
use crate::cloud_provider::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
use crate::cloud_provider::helm_charts::qovery_cluster_agent_chart::QoveryClusterAgentChart;
use crate::cloud_provider::helm_charts::qovery_pdb_infra_chart::QoveryPdbInfraChart;
use crate::cloud_provider::helm_charts::qovery_priority_class_chart::QoveryPriorityClassChart;
use crate::cloud_provider::helm_charts::qovery_shell_agent_chart::QoveryShellAgentChart;
use crate::cloud_provider::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartResources, HelmChartResourcesConstraintType, HelmChartTimeout,
    ToCommonHelmChart,
};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::models::{
    CpuArchitecture, CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit,
};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::Kind;
use crate::cloud_provider::Kind as CloudProviderKind;
use crate::dns_provider::DnsProviderConfiguration;
use crate::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use crate::errors::CommandError;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::QoveryIdentifier;
use crate::models::domain::Domain;
use crate::models::gcp::{GcpStorageType, JsonCredentials};
use crate::models::third_parties::LetsEncryptConfig;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use time::Duration;
use url::Url;

pub struct ChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub region: GcpRegion,
    pub cluster_name: String,
    pub cpu_architectures: Vec<CpuArchitecture>,
    pub cloud_provider: String,
    pub test_cluster: bool,
    pub gcp_credentials: JsonCredentials,
    pub qovery_engine_location: EngineLocation,
    pub ff_log_history_enabled: bool,
    pub ff_metrics_history_enabled: bool,
    pub ff_grafana_enabled: bool,
    pub managed_dns_name: String,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub managed_dns_root_domain_helm_format: String,
    pub external_dns_provider: String,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub dns_provider_config: DnsProviderConfiguration,
    pub loki_logging_service_account_email: String,
    pub logs_bucket_name: String,
    // qovery options form json input
    pub infra_options: GkeOptions,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
}

impl ChartsConfigPrerequisites {
    pub fn new(
        organization_id: String,
        organization_long_id: uuid::Uuid,
        cluster_id: String,
        cluster_long_id: uuid::Uuid,
        region: GcpRegion,
        cluster_name: String,
        cpu_architectures: Vec<CpuArchitecture>,
        cloud_provider: String,
        test_cluster: bool,
        gcp_credentials: JsonCredentials,
        qovery_engine_location: EngineLocation,
        ff_log_history_enabled: bool,
        ff_metrics_history_enabled: bool,
        ff_grafana_enabled: bool,
        managed_dns_name: String,
        managed_dns_helm_format: String,
        managed_dns_resolvers_terraform_format: String,
        managed_dns_root_domain_helm_format: String,
        external_dns_provider: String,
        lets_encrypt_config: LetsEncryptConfig,
        dns_provider_config: DnsProviderConfiguration,
        loki_logging_service_account_email: String,
        logs_bucket_name: String,
        infra_options: GkeOptions,
        cluster_advanced_settings: ClusterAdvancedSettings,
    ) -> Self {
        Self {
            organization_id,
            organization_long_id,
            cluster_id,
            cluster_long_id,
            region,
            cluster_name,
            cpu_architectures,
            cloud_provider,
            test_cluster,
            gcp_credentials,
            qovery_engine_location,
            ff_log_history_enabled,
            ff_metrics_history_enabled,
            ff_grafana_enabled,
            managed_dns_name,
            managed_dns_helm_format,
            managed_dns_resolvers_terraform_format,
            managed_dns_root_domain_helm_format,
            external_dns_provider,
            lets_encrypt_config,
            dns_provider_config,
            loki_logging_service_account_email,
            logs_bucket_name,
            infra_options,
            cluster_advanced_settings,
        }
    }
}

pub fn gcp_helm_charts(
    qovery_terraform_config_file: &str,
    chart_config_prerequisites: &ChartsConfigPrerequisites,
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

    let _config_file = match File::open(qovery_terraform_config_file) {
        Ok(x) => x,
        Err(e) => {
            return Err(CommandError::new(
                "Can't deploy helm chart as Qovery terraform config file has not been rendered by Terraform. Are you running it in dry run mode?".to_string(),
                Some(e.to_string()),
                Some(envs.to_vec()),
            ));
        }
    };
    let prometheus_namespace = HelmChartNamespaces::Qovery;
    let prometheus_internal_url = format!("http://prometheus-operated.{prometheus_namespace}.svc");
    let loki_namespace = HelmChartNamespaces::Qovery;
    let loki_kube_dns_name = format!("loki.{loki_namespace}.svc:3100");

    // Qovery storage class
    let q_storage_class_chart = QoveryStorageClassChart::new(
        chart_prefix_path,
        CloudProviderKind::Gcp,
        HashSet::from_iter(vec![QoveryStorageType::Ssd, QoveryStorageType::Hdd]), // TODO(ENG-1800): Add Cold and Nvme
        HelmChartNamespaces::Qovery, // Cannot install anything inside kube-system namespace when it comes to GKE autopilot
    )
    .to_common_helm_chart()?;

    // Qovery priority class
    let q_priority_class_chart = QoveryPriorityClassChart::new(
        chart_prefix_path,
        HashSet::from_iter(vec![QoveryPriorityClass::HighPriority]), // Cannot use node critical priority class on GKE autopilot
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
            )
            .to_common_helm_chart()?,
        )),
    };

    // Kube prometheus stack
    let kube_prometheus_stack: Option<Box<dyn HelmChart>> = match chart_config_prerequisites.ff_metrics_history_enabled
    {
        false => None,
        true => Some(Box::new(
            KubePrometheusStackChart::new(
                chart_prefix_path,
                GcpStorageType::Balanced.to_k8s_storage_class(),
                prometheus_internal_url.to_string(),
                prometheus_namespace,
                true,
                get_chart_override_fn.clone(),
                true,
            )
            .to_common_helm_chart()?,
        )),
    };

    // Prometheus adapter
    let prometheus_adapter: Option<Box<dyn HelmChart>> = match chart_config_prerequisites.ff_metrics_history_enabled {
        false => None,
        true => Some(Box::new(
            PrometheusAdapterChart::new(
                chart_prefix_path,
                prometheus_internal_url.clone(),
                prometheus_namespace,
                get_chart_override_fn.clone(),
                true,
            )
            .to_common_helm_chart()?,
        )),
    };

    // Kube state metrics
    let kube_state_metrics: Option<Box<dyn HelmChart>> = match chart_config_prerequisites.ff_metrics_history_enabled {
        false => None,
        true => Some(Box::new(
            KubeStateMetricsChart::new(
                chart_prefix_path,
                HelmChartNamespaces::Qovery,
                true,
                get_chart_override_fn.clone(),
            )
            .to_common_helm_chart()?,
        )),
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
        chart_config_prerequisites.ff_metrics_history_enabled,
        get_chart_override_fn.clone(),
        domain.clone(),
        Kind::Gcp,
        chart_config_prerequisites.organization_long_id.to_string(),
        chart_config_prerequisites.organization_id.clone(),
        chart_config_prerequisites.cluster_long_id.to_string(),
        chart_config_prerequisites.cluster_id.clone(),
        KubernetesKind::Gke,
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
        HelmChartNamespaces::Qovery,
        None,
        chart_config_prerequisites
            .cluster_advanced_settings
            .nginx_controller_enable_client_ip,
        chart_config_prerequisites
            .cluster_advanced_settings
            .nginx_controller_log_format_escaping
            .to_model(),
        false, // only for AWS
    )
    .to_common_helm_chart()?;

    // K8s Event Logger
    let k8s_event_logger =
        K8sEventLoggerChart::new(chart_prefix_path, true, HelmChartNamespaces::Qovery).to_common_helm_chart()?;

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

    // chart deployment order matters!!!
    // Helm chart deployment order
    let level_1: Vec<Option<Box<dyn HelmChart>>> = vec![
        Some(Box::new(q_storage_class_chart)),
        Some(Box::new(q_priority_class_chart)),
        promtail,
        kube_prometheus_stack,
    ];
    let level_2: Vec<Option<Box<dyn HelmChart>>> = vec![loki, prometheus_adapter, kube_state_metrics];
    let level_3: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(cert_manager))];
    let level_4: Vec<Option<Box<dyn HelmChart>>> = vec![qovery_cert_manager_webhook];
    let level_5: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(external_dns_chart))];
    let level_6: Vec<Option<Box<dyn HelmChart>>> = vec![Some(Box::new(nginx_ingress))];
    let mut level_7: Vec<Option<Box<dyn HelmChart>>> = vec![
        Some(Box::new(cert_manager_config)),
        Some(Box::new(qovery_cluster_agent)),
        Some(Box::new(qovery_shell_agent)),
        Some(Box::new(k8s_event_logger)),
    ];

    // pdb infra
    if chart_config_prerequisites.cluster_advanced_settings.infra_pdb_enabled {
        let pdb_infra = QoveryPdbInfraChart::new(
            chart_prefix_path,
            HelmChartNamespaces::Qovery,
            prometheus_namespace,
            loki_namespace,
        )
        .to_common_helm_chart()?;

        level_7.push(Some(Box::new(pdb_infra)));
    }

    Ok(vec![
        level_1.into_iter().flatten().collect(),
        level_2.into_iter().flatten().collect(),
        level_3.into_iter().flatten().collect(),
        level_4.into_iter().flatten().collect(),
        level_5.into_iter().flatten().collect(),
        level_6.into_iter().flatten().collect(),
        level_7.into_iter().flatten().collect(),
    ])
}
