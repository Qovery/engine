use crate::cloud_provider::helm::{
    get_chart_for_cert_manager_config, get_chart_for_cluster_agent, get_chart_for_shell_agent,
    get_engine_helm_action_from_location, ChartInfo, ChartSetValue, ChartValuesGenerated, ClusterAgentContext,
    CommonChart, HelmAction, HelmChart, HelmChartNamespaces, ShellAgentContext,
};
use crate::cloud_provider::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::cloud_provider::helm_charts::ToCommonHelmChart;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::qovery::{get_qovery_app_version, EngineLocation, QoveryAppName, QoveryEngine};
use crate::cloud_provider::scaleway::kubernetes::KapsuleOptions;

use crate::dns_provider::DnsProviderConfiguration;
use crate::errors::CommandError;
use crate::models::scaleway::{ScwRegion, ScwZone};

use crate::cloud_provider::helm_charts::core_dns_config_chart::CoreDNSConfigChart;
use crate::cloud_provider::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::cloud_provider::helm_charts::kube_prometheus_stack_chart::KubePrometheusStackChart;
use crate::cloud_provider::helm_charts::loki_chart::{LokiChart, LokiEncryptionType, LokiS3BucketConfiguration};
use crate::cloud_provider::helm_charts::promtail_chart::PromtailChart;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::iter::FromIterator;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalewayQoveryTerraformConfig {
    pub loki_storage_config_scaleway_s3: String,
}

pub struct ChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub zone: ScwZone,
    pub region: ScwRegion,
    pub cluster_name: String,
    pub cloud_provider: String,
    pub test_cluster: bool,
    pub scw_access_key: String,
    pub scw_secret_key: String,
    pub scw_project_id: String,
    pub qovery_engine_location: EngineLocation,
    pub ff_log_history_enabled: bool,
    pub ff_metrics_history_enabled: bool,
    pub managed_dns_name: String,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub managed_dns_root_domain_helm_format: String,
    pub external_dns_provider: String,
    pub dns_email_report: String,
    pub acme_url: String,
    pub dns_provider_config: DnsProviderConfiguration,
    pub disable_pleco: bool,
    // qovery options form json input
    pub infra_options: KapsuleOptions,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
}

impl ChartsConfigPrerequisites {
    pub fn new(
        organization_id: String,
        organization_long_id: uuid::Uuid,
        cluster_id: String,
        cluster_long_id: uuid::Uuid,
        zone: ScwZone,
        cluster_name: String,
        cloud_provider: String,
        test_cluster: bool,
        scw_access_key: String,
        scw_secret_key: String,
        scw_project_id: String,
        qovery_engine_location: EngineLocation,
        ff_log_history_enabled: bool,
        ff_metrics_history_enabled: bool,
        managed_dns_name: String,
        managed_dns_helm_format: String,
        managed_dns_resolvers_terraform_format: String,
        managed_dns_root_domain_helm_format: String,
        external_dns_provider: String,
        dns_email_report: String,
        acme_url: String,
        dns_provider_config: DnsProviderConfiguration,
        disable_pleco: bool,
        infra_options: KapsuleOptions,
        cluster_advanced_settings: ClusterAdvancedSettings,
    ) -> Self {
        ChartsConfigPrerequisites {
            organization_id,
            organization_long_id,
            cluster_id,
            cluster_long_id,
            zone,
            region: zone.region(),
            cluster_name,
            cloud_provider,
            test_cluster,
            scw_access_key,
            scw_secret_key,
            scw_project_id,
            qovery_engine_location,
            ff_log_history_enabled,
            ff_metrics_history_enabled,
            managed_dns_name,
            managed_dns_helm_format,
            managed_dns_resolvers_terraform_format,
            managed_dns_root_domain_helm_format,
            external_dns_provider,
            dns_email_report,
            acme_url,
            dns_provider_config,
            disable_pleco,
            infra_options,
            cluster_advanced_settings,
        }
    }
}

pub fn scw_helm_charts(
    qovery_terraform_config_file: &str,
    chart_config_prerequisites: &ChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    _kubernetes_config: &Path,
    envs: &[(String, String)],
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
    info!("preparing chart configuration to be deployed");

    let content_file = match File::open(&qovery_terraform_config_file) {
        Ok(x) => x,
        Err(e) => {
            return Err(CommandError::new(
                "Can't deploy helm chart as Qovery terraform config file has not been rendered by Terraform. Are you running it in dry run mode?".to_string(),
                Some(e.to_string()),
                Some(envs.to_vec()),
            ));
        }
    };
    let chart_prefix = chart_prefix_path.unwrap_or("./");
    let chart_path = |x: &str| -> String { format!("{}/{}", &chart_prefix, x) };
    let reader = BufReader::new(content_file);
    let qovery_terraform_config: ScalewayQoveryTerraformConfig = match serde_json::from_reader(reader) {
        Ok(config) => config,
        Err(e) => {
            return Err(CommandError::new(
                format!("Error while parsing terraform config file {}", qovery_terraform_config_file),
                Some(e.to_string()),
                Some(envs.to_vec()),
            ));
        }
    };

    let prometheus_namespace = HelmChartNamespaces::Prometheus;
    let prometheus_internal_url = format!("http://prometheus-operated.{}.svc", prometheus_namespace);
    let loki_namespace = HelmChartNamespaces::Logging;
    let loki_kube_dns_name = format!("loki.{}.svc:3100", loki_namespace);

    // Qovery storage class
    let q_storage_class =
        QoveryStorageClassChart::new(chart_prefix_path, HashSet::from_iter(vec![QoveryStorageType::Ssd]))
            .to_common_helm_chart();

    // CoreDNS config
    let coredns_config = CoreDNSConfigChart::new(
        chart_prefix_path,
        vec![
            "addonmanager.kubernetes.io/mode: EnsureExists".to_string(),
            "k8s.scw.cloud/object: ConfigMap".to_string(),
            "k8s.scw.cloud/system: coredns".to_string(),
            "k8s-app: kube-dns".to_string(),
        ],
        false,
        chart_config_prerequisites.managed_dns_helm_format.to_string(),
        chart_config_prerequisites
            .managed_dns_resolvers_terraform_format
            .to_string(),
    );

    // External DNS
    let external_dns = ExternalDNSChart::new(
        chart_prefix_path,
        chart_config_prerequisites.dns_provider_config.clone(),
        chart_config_prerequisites
            .managed_dns_root_domain_helm_format
            .to_string(),
        false,
        chart_config_prerequisites.cluster_id.to_string(),
    )
    .to_common_helm_chart();

    // Promtail
    let promtail = PromtailChart::new(chart_prefix_path, loki_kube_dns_name).to_common_helm_chart();

    // Loki
    let loki = LokiChart::new(
        chart_prefix_path,
        LokiEncryptionType::None, // Scaleway does not support encryption yet.
        loki_namespace,
        chart_config_prerequisites
            .cluster_advanced_settings
            .loki_log_retention_in_week,
        LokiS3BucketConfiguration {
            s3_config: Some(qovery_terraform_config.loki_storage_config_scaleway_s3),
            use_path_style: true,
            region: Some(chart_config_prerequisites.zone.region().to_string()),
            ..Default::default()
        },
    )
    .to_common_helm_chart();

    /* Example to delete an old chart
    let old_prometheus_operator = PrometheusOperatorConfigChart {
        chart_info: ChartInfo {
            name: "prometheus-operator".to_string(),
            namespace: prometheus_namespace,
            action: HelmAction::Destroy,
            ..Default::default()
        },
    };*/

    // Kube prometheus stack
    let kube_prometheus_stack = KubePrometheusStackChart::new(
        chart_prefix_path,
        "scw-sbv-ssd-0".to_string(),
        prometheus_internal_url.to_string(),
        prometheus_namespace,
        true,
    )
    .to_common_helm_chart();

    let prometheus_adapter = CommonChart {
        chart_info: ChartInfo {
            name: "prometheus-adapter".to_string(),
            path: chart_path("common/charts/prometheus-adapter"),
            last_breaking_version_requiring_restart: Some(Version::new(3, 3, 1)),
            namespace: prometheus_namespace,
            values: vec![
                ChartSetValue {
                    key: "metricsRelistInterval".to_string(),
                    value: "30s".to_string(),
                },
                ChartSetValue {
                    key: "prometheus.url".to_string(),
                    value: prometheus_internal_url.clone(),
                },
                ChartSetValue {
                    key: "podDisruptionBudget.enabled".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "podDisruptionBudget.maxUnavailable".to_string(),
                    value: "1".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "250m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "250m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "384Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "384Mi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    // metric-server is built-in Scaleway cluster, no need to manage it

    let kube_state_metrics = CommonChart {
        chart_info: ChartInfo {
            name: "kube-state-metrics".to_string(),
            namespace: HelmChartNamespaces::Prometheus,
            last_breaking_version_requiring_restart: Some(Version::new(4, 6, 0)),
            path: chart_path("common/charts/kube-state-metrics"),
            values: vec![
                ChartSetValue {
                    key: "prometheus.monitor.enabled".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "75m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "75m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "384Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "384Mi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    let grafana_datasources = format!(
        "
datasources:
  datasources.yaml:
    apiVersion: 1
    datasources:
      - name: Prometheus
        type: prometheus
        url: \"{}:9090\"
        access: proxy
        isDefault: true
      - name: PromLoki
        type: prometheus
        url: \"http://{}.{}.svc:3100/loki\"
        access: proxy
        isDefault: false
      - name: Loki
        type: loki
        url: \"http://{}.{}.svc:3100\"
      ",
        prometheus_internal_url, &loki.chart_info.name, loki_namespace, &loki.chart_info.name, loki_namespace,
    );

    let grafana = CommonChart {
        chart_info: ChartInfo {
            name: "grafana".to_string(),
            path: chart_path("common/charts/grafana"),
            namespace: prometheus_namespace,
            values_files: vec![chart_path("chart_values/grafana.yaml")],
            yaml_files_content: vec![ChartValuesGenerated {
                filename: "grafana_generated.yaml".to_string(),
                yaml_content: grafana_datasources,
            }],
            ..Default::default()
        },
        ..Default::default()
    };

    let cert_manager = CommonChart {
        chart_info: ChartInfo {
            name: "cert-manager".to_string(),
            path: chart_path("common/charts/cert-manager"),
            namespace: HelmChartNamespaces::CertManager,
            last_breaking_version_requiring_restart: Some(Version::new(1, 4, 4)),
            values: vec![
                ChartSetValue {
                    key: "installCRDs".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "startupapicheck.jobAnnotations.helm\\.sh/hook".to_string(),
                    value: "post-install\\,post-upgrade".to_string(),
                },
                ChartSetValue {
                    key: "startupapicheck.rbac.annotations.helm\\.sh/hook".to_string(),
                    value: "post-install\\,post-upgrade".to_string(),
                },
                ChartSetValue {
                    key: "startupapicheck.serviceAccount.annotations.helm\\.sh/hook".to_string(),
                    value: "post-install\\,post-upgrade".to_string(),
                },
                ChartSetValue {
                    key: "replicaCount".to_string(),
                    value: "1".to_string(),
                },
                // https://cert-manager.io/docs/configuration/acme/dns01/#setting-nameservers-for-dns01-self-check
                ChartSetValue {
                    key: "extraArgs".to_string(),
                    value: "{--dns01-recursive-nameservers-only,--dns01-recursive-nameservers=1.1.1.1:53\\,8.8.8.8:53}"
                        .to_string(),
                },
                ChartSetValue {
                    key: "prometheus.servicemonitor.enabled".to_string(),
                    value: chart_config_prerequisites.ff_metrics_history_enabled.to_string(),
                },
                ChartSetValue {
                    key: "prometheus.servicemonitor.prometheusInstance".to_string(),
                    value: "qovery".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "1Gi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "1Gi".to_string(),
                },
                // Webhooks resources limits
                ChartSetValue {
                    key: "webhook.resources.limits.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "webhook.resources.requests.cpu".to_string(),
                    value: "50m".to_string(),
                },
                ChartSetValue {
                    key: "webhook.resources.limits.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                ChartSetValue {
                    key: "webhook.resources.requests.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                // Cainjector resources limits
                ChartSetValue {
                    key: "cainjector.resources.limits.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "cainjector.resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "cainjector.resources.limits.memory".to_string(),
                    value: "1Gi".to_string(),
                },
                ChartSetValue {
                    key: "cainjector.resources.requests.memory".to_string(),
                    value: "1Gi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    let cert_manager_config = get_chart_for_cert_manager_config(
        &chart_config_prerequisites.dns_provider_config,
        chart_path("common/charts/cert-manager-configs"),
        chart_config_prerequisites.dns_email_report.clone(),
        chart_config_prerequisites.acme_url.clone(),
        chart_config_prerequisites.managed_dns_helm_format.clone(),
    );

    let mut qovery_cert_manager_webhook: Option<CommonChart> = None;
    if let DnsProviderConfiguration::QoveryDns(qovery_dns_config) = &chart_config_prerequisites.dns_provider_config {
        qovery_cert_manager_webhook = Some(CommonChart {
            chart_info: ChartInfo {
                name: "qovery-cert-manager-webhook".to_string(),
                namespace: HelmChartNamespaces::CertManager,
                path: chart_path("common/charts/qovery-cert-manager-webhook"),
                values: vec![
                    ChartSetValue {
                        key: "secret.apiKey".to_string(),
                        value: qovery_dns_config.api_key.to_string(),
                    },
                    ChartSetValue {
                        key: "secret.apiUrl".to_string(),
                        value: qovery_dns_config.api_url.to_string(), // URL standard port will be omitted from string as standard (80 HTTP & 443 HTTPS)
                    },
                    ChartSetValue {
                        key: "certManager.serviceAccountName".to_string(),
                        value: "cert-manager".to_string(),
                    },
                    ChartSetValue {
                        key: "certManager.namespace".to_string(),
                        value: HelmChartNamespaces::CertManager.to_string(),
                    },
                    // resources limits
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: "48Mi".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: "48Mi".to_string(),
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        });
    }

    let nginx_ingress = CommonChart {
        chart_info: ChartInfo {
            name: "nginx-ingress".to_string(),
            path: chart_path("common/charts/ingress-nginx"),
            namespace: HelmChartNamespaces::NginxIngress,
            // Because of NLB, svc can take some time to start
            timeout_in_seconds: 300,
            values_files: vec![chart_path("chart_values/nginx-ingress.yaml")],
            values: vec![
                ChartSetValue {
                    key: "controller.admissionWebhooks.enabled".to_string(),
                    value: "false".to_string(),
                },
                // Controller resources limits
                ChartSetValue {
                    key: "controller.resources.limits.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "controller.resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "controller.resources.limits.memory".to_string(),
                    value: "768Mi".to_string(),
                },
                ChartSetValue {
                    key: "controller.resources.requests.memory".to_string(),
                    value: "768Mi".to_string(),
                },
                // Default backend resources limits
                ChartSetValue {
                    key: "defaultBackend.resources.limits.cpu".to_string(),
                    value: "20m".to_string(),
                },
                ChartSetValue {
                    key: "defaultBackend.resources.requests.cpu".to_string(),
                    value: "10m".to_string(),
                },
                ChartSetValue {
                    key: "defaultBackend.resources.limits.memory".to_string(),
                    value: "32Mi".to_string(),
                },
                ChartSetValue {
                    key: "defaultBackend.resources.requests.memory".to_string(),
                    value: "32Mi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    let pleco = CommonChart {
        chart_info: ChartInfo {
            name: "pleco".to_string(),
            path: chart_path("common/charts/pleco"),
            values_files: vec![chart_path("chart_values/pleco-scw.yaml")],
            values: vec![
                ChartSetValue {
                    key: "environmentVariables.SCW_ACCESS_KEY".to_string(),
                    value: chart_config_prerequisites.scw_access_key.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.SCW_SECRET_KEY".to_string(),
                    value: chart_config_prerequisites.scw_secret_key.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.SCW_VOLUME_TIMEOUT".to_string(),
                    value: 24i32.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.LOG_LEVEL".to_string(),
                    value: "debug".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    let cluster_agent_context = ClusterAgentContext {
        api_url: &chart_config_prerequisites.infra_options.qovery_api_url,
        api_token: &chart_config_prerequisites.infra_options.agent_version_controller_token,
        organization_long_id: &chart_config_prerequisites.organization_long_id,
        cluster_id: &chart_config_prerequisites.cluster_id,
        cluster_long_id: &chart_config_prerequisites.cluster_long_id,
        cluster_jwt_token: &chart_config_prerequisites.infra_options.jwt_token,
        grpc_url: &chart_config_prerequisites.infra_options.qovery_grpc_url,
        loki_url: if chart_config_prerequisites.ff_log_history_enabled {
            Some("http://loki.logging.svc.cluster.local:3100")
        } else {
            None
        },
    };
    let cluster_agent = get_chart_for_cluster_agent(cluster_agent_context, chart_path, None)?;

    let shell_context = ShellAgentContext {
        api_url: &chart_config_prerequisites.infra_options.qovery_api_url,
        api_token: &chart_config_prerequisites.infra_options.agent_version_controller_token,
        organization_long_id: &chart_config_prerequisites.organization_long_id,
        cluster_id: &chart_config_prerequisites.cluster_id,
        cluster_long_id: &chart_config_prerequisites.cluster_long_id,
        cluster_jwt_token: &chart_config_prerequisites.infra_options.jwt_token,
        grpc_url: &chart_config_prerequisites.infra_options.qovery_grpc_url,
    };
    let shell_agent = get_chart_for_shell_agent(shell_context, chart_path, None)?;

    let qovery_agent = CommonChart {
        chart_info: ChartInfo {
            name: "qovery-agent".to_string(),
            path: chart_path("common/charts/qovery/qovery-agent"),
            namespace: HelmChartNamespaces::Qovery,
            action: HelmAction::Destroy,
            ..Default::default()
        },
        ..Default::default()
    };

    let qovery_engine_version: QoveryEngine = get_qovery_app_version(
        QoveryAppName::Engine,
        &chart_config_prerequisites.infra_options.engine_version_controller_token,
        &chart_config_prerequisites.infra_options.qovery_api_url,
        &chart_config_prerequisites.cluster_id,
    )?;

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
                    value: qovery_engine_version.version,
                },
                ChartSetValue {
                    key: "autoscaler.min_replicas".to_string(),
                    value: "2".to_string(),
                },
                ChartSetValue {
                    key: "metrics.enabled".to_string(),
                    value: chart_config_prerequisites.ff_metrics_history_enabled.to_string(),
                },
                ChartSetValue {
                    key: "volumes.storageClassName".to_string(),
                    value: "scw-sbv-ssd-0".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.QOVERY_NATS_URL".to_string(),
                    value: chart_config_prerequisites.infra_options.qovery_nats_url.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.QOVERY_NATS_USER".to_string(),
                    value: chart_config_prerequisites.infra_options.qovery_nats_user.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.QOVERY_NATS_PASSWORD".to_string(),
                    value: chart_config_prerequisites
                        .infra_options
                        .qovery_nats_password
                        .to_string(),
                },
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
                // engine resources limits
                ChartSetValue {
                    key: "engineResources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.requests.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.limits.memory".to_string(),
                    value: "512Mi".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.requests.memory".to_string(),
                    value: "512Mi".to_string(),
                },
                // build resources limits
                ChartSetValue {
                    key: "buildResources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "buildResources.requests.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "buildResources.limits.memory".to_string(),
                    value: "4Gi".to_string(),
                },
                ChartSetValue {
                    key: "buildResources.requests.memory".to_string(),
                    value: "4Gi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    // chart deployment order matters!!!
    let mut level_1: Vec<Box<dyn HelmChart>> = vec![Box::new(q_storage_class), Box::new(coredns_config)];

    let mut level_2: Vec<Box<dyn HelmChart>> = vec![];

    let level_3: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager)];

    let level_4: Vec<Box<dyn HelmChart>> = if let Some(qovery_webhook) = qovery_cert_manager_webhook {
        vec![Box::new(qovery_webhook)]
    } else {
        vec![]
    };

    let level_5: Vec<Box<dyn HelmChart>> = vec![Box::new(external_dns)];

    let mut level_6: Vec<Box<dyn HelmChart>> = vec![Box::new(nginx_ingress)];

    let level_7: Vec<Box<dyn HelmChart>> = vec![
        Box::new(cert_manager_config),
        Box::new(cluster_agent),
        Box::new(qovery_agent), // Old agent, this one should be removed/migrated
        Box::new(shell_agent),
        Box::new(qovery_engine),
    ];

    // // observability
    if chart_config_prerequisites.ff_metrics_history_enabled {
        level_1.push(Box::new(kube_prometheus_stack));
        level_2.push(Box::new(prometheus_adapter));
        level_2.push(Box::new(kube_state_metrics));
    }
    if chart_config_prerequisites.ff_log_history_enabled {
        level_1.push(Box::new(promtail));
        level_2.push(Box::new(loki));
    }

    if chart_config_prerequisites.ff_metrics_history_enabled || chart_config_prerequisites.ff_log_history_enabled {
        level_2.push(Box::new(grafana))
    };

    // pleco
    if !chart_config_prerequisites.disable_pleco {
        level_6.push(Box::new(pleco));
    }

    info!("charts configuration preparation finished");
    Ok(vec![level_1, level_2, level_3, level_4, level_5, level_6, level_7])
}
