use crate::cloud_provider::aws::kubernetes::{Options, VpcQoveryNetworkMode};
use crate::cloud_provider::helm::{
    get_chart_for_cluster_agent, get_chart_for_shell_agent, get_engine_helm_action_from_location, ChartInfo,
    ChartSetValue, ClusterAgentContext, CommonChart, HelmAction, HelmChart, HelmChartNamespaces, ShellAgentContext,
    UpdateStrategy,
};
use crate::cloud_provider::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::cloud_provider::helm_charts::{HelmChartResourcesConstraintType, ToCommonHelmChart};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::models::CpuArchitecture;
use crate::cloud_provider::qovery::EngineLocation;

use crate::dns_provider::DnsProviderConfiguration;
use crate::errors::CommandError;

use crate::cloud_provider::aws::kubernetes::helm_charts::aws_iam_eks_user_mapper_chart::AwsIamEksUserMapperChart;
use crate::cloud_provider::aws::kubernetes::helm_charts::aws_node_term_handler_chart::AwsNodeTermHandlerChart;
use crate::cloud_provider::aws::kubernetes::helm_charts::aws_ui_view_chart::AwsUiViewChart;
use crate::cloud_provider::aws::kubernetes::helm_charts::cluster_autoscaler_chart::ClusterAutoscalerChart;
use crate::cloud_provider::helm_charts::cert_manager_chart::CertManagerChart;
use crate::cloud_provider::helm_charts::cert_manager_config_chart::CertManagerConfigsChart;
use crate::cloud_provider::helm_charts::coredns_config_chart::CoreDNSConfigChart;
use crate::cloud_provider::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::cloud_provider::helm_charts::grafana_chart::{
    CloudWatchConfig, GrafanaAdminUser, GrafanaChart, GrafanaDatasources,
};
use crate::cloud_provider::helm_charts::kube_prometheus_stack_chart::KubePrometheusStackChart;
use crate::cloud_provider::helm_charts::kube_state_metrics::KubeStateMetricsChart;
use crate::cloud_provider::helm_charts::loki_chart::{LokiChart, LokiEncryptionType, LokiS3BucketConfiguration};
use crate::cloud_provider::helm_charts::metrics_server_chart::MetricsServerChart;
use crate::cloud_provider::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
use crate::cloud_provider::helm_charts::promtail_chart::PromtailChart;
use crate::cloud_provider::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
use crate::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use crate::models::aws::AwsStorageType;
use crate::models::third_parties::LetsEncryptConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::iter::FromIterator;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsEksQoveryTerraformConfig {
    pub aws_iam_eks_user_mapper_key: String,
    pub aws_iam_eks_user_mapper_secret: String,
    pub aws_iam_cluster_autoscaler_role_arn: String,
    pub aws_iam_cloudwatch_role_arn: String,
    pub aws_iam_loki_role_arn: String,
    pub aws_s3_loki_bucket_name: String,
    pub loki_storage_config_aws_s3: String,
}

pub struct EksChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub region: String,
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
    pub managed_dns_name: String,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub managed_dns_root_domain_helm_format: String,
    pub external_dns_provider: String,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub dns_provider_config: DnsProviderConfiguration,
    pub disable_pleco: bool,
    // qovery options form json input
    pub infra_options: Options,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
}

pub fn eks_aws_helm_charts(
    qovery_terraform_config_file: &str,
    chart_config_prerequisites: &EksChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    _kubernetes_config: &Path,
    envs: &[(String, String)],
    qovery_api: &dyn QoveryApi,
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
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
    let chart_prefix = chart_prefix_path.unwrap_or("./");
    let chart_path = |x: &str| -> String { format!("{}/{}", &chart_prefix, x) };
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

    let prometheus_namespace = HelmChartNamespaces::Prometheus;
    let prometheus_internal_url = format!("http://prometheus-operated.{prometheus_namespace}.svc");
    let loki_namespace = HelmChartNamespaces::Logging;
    let loki_kube_dns_name = format!("loki.{loki_namespace}.svc:3100");

    // Qovery storage class
    let q_storage_class = QoveryStorageClassChart::new(
        chart_prefix_path,
        HashSet::from_iter(vec![
            QoveryStorageType::Ssd,
            QoveryStorageType::Hdd,
            QoveryStorageType::Cold,
            QoveryStorageType::Nvme,
        ]),
    )
    .to_common_helm_chart();

    // AWS IAM EKS user mapper
    let aws_iam_eks_user_mapper = AwsIamEksUserMapperChart::new(
        chart_prefix_path,
        chart_config_prerequisites.region.to_string(),
        qovery_terraform_config.aws_iam_eks_user_mapper_key,
        qovery_terraform_config.aws_iam_eks_user_mapper_secret,
        chart_config_prerequisites
            .cluster_advanced_settings
            .aws_iam_user_mapper_group_name
            .to_string(),
    )
    .to_common_helm_chart();

    // AWS nodes term handler
    let aws_node_term_handler = AwsNodeTermHandlerChart::new(chart_prefix_path).to_common_helm_chart();

    // AWS UI view
    let aws_ui_view = AwsUiViewChart::new(chart_prefix_path).to_common_helm_chart();

    // Cluster autoscaler
    let cluster_autoscaler = ClusterAutoscalerChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cloud_provider.to_string(),
        chart_config_prerequisites.region.to_string(),
        chart_config_prerequisites.cluster_name.to_string(),
        qovery_terraform_config.aws_iam_cluster_autoscaler_role_arn.to_string(),
        prometheus_namespace,
        chart_config_prerequisites.ff_metrics_history_enabled,
    )
    .to_common_helm_chart();

    // CoreDNS config
    let coredns_config = CoreDNSConfigChart::new(
        chart_prefix_path,
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
        UpdateStrategy::RollingUpdate,
    )
    .to_common_helm_chart();

    // Promtail
    let promtail = match chart_config_prerequisites.ff_log_history_enabled {
        false => None,
        true => Some(PromtailChart::new(chart_prefix_path, loki_kube_dns_name).to_common_helm_chart()),
    };

    // Loki
    let loki = match chart_config_prerequisites.ff_log_history_enabled {
        false => None,
        true => Some(
            LokiChart::new(
                chart_prefix_path,
                LokiEncryptionType::ServerSideEncryption,
                loki_namespace,
                chart_config_prerequisites
                    .cluster_advanced_settings
                    .loki_log_retention_in_week,
                LokiS3BucketConfiguration {
                    region: Some(chart_config_prerequisites.region.to_string()),
                    bucketname: Some(qovery_terraform_config.aws_s3_loki_bucket_name),
                    s3_config: Some(qovery_terraform_config.loki_storage_config_aws_s3),
                    aws_iam_loki_role_arn: Some(qovery_terraform_config.aws_iam_loki_role_arn),
                    ..Default::default()
                },
            )
            .to_common_helm_chart(),
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

    // Kube prometheus stack
    let kube_prometheus_stack = match chart_config_prerequisites.ff_metrics_history_enabled {
        false => None,
        true => Some(
            KubePrometheusStackChart::new(
                chart_prefix_path,
                AwsStorageType::GP2.to_k8s_storage_class(),
                prometheus_internal_url.to_string(),
                prometheus_namespace,
                false,
            )
            .to_common_helm_chart(),
        ),
    };

    // Prometheus adapter
    let prometheus_adapter = match chart_config_prerequisites.ff_metrics_history_enabled {
        false => None,
        true => Some(
            PrometheusAdapterChart::new(chart_prefix_path, prometheus_internal_url.clone(), prometheus_namespace)
                .to_common_helm_chart(),
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
            )
            .to_common_helm_chart(),
        );
    }

    // Metrics server
    let metrics_server = MetricsServerChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::ChartDefault,
        UpdateStrategy::RollingUpdate,
    )
    .to_common_helm_chart();

    // Kube state metrics
    let kube_state_metrics = match chart_config_prerequisites.ff_metrics_history_enabled {
        false => None,
        true => Some(KubeStateMetricsChart::new(chart_prefix_path).to_common_helm_chart()),
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
                        chart_config_prerequisites.region.to_string(),
                        qovery_terraform_config.aws_iam_cloudwatch_role_arn,
                    )),
                },
                AwsStorageType::GP2.to_k8s_storage_class(),
            )
            .to_common_helm_chart(),
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
    )
    .to_common_helm_chart();

    // Cert Manager Configs
    let cert_manager_config = CertManagerConfigsChart::new(
        chart_prefix_path,
        &chart_config_prerequisites.lets_encrypt_config,
        &chart_config_prerequisites.dns_provider_config,
        chart_config_prerequisites.managed_dns_helm_format.to_string(),
    )
    .to_common_helm_chart();

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
                // metrics
                ChartSetValue {
                    key: "controller.metrics.enabled".to_string(),
                    value: chart_config_prerequisites.ff_metrics_history_enabled.to_string(),
                },
                ChartSetValue {
                    key: "controller.metrics.serviceMonitor.enabled".to_string(),
                    value: chart_config_prerequisites.ff_metrics_history_enabled.to_string(),
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

    let pleco = match chart_config_prerequisites.disable_pleco {
        true => None,
        false => Some(CommonChart {
            chart_info: ChartInfo {
                name: "pleco".to_string(),
                path: chart_path("common/charts/pleco"),
                values_files: vec![chart_path("chart_values/pleco-aws.yaml")],
                values: vec![
                    ChartSetValue {
                        key: "environmentVariables.AWS_ACCESS_KEY_ID".to_string(),
                        value: chart_config_prerequisites.aws_access_key_id.clone(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.AWS_SECRET_ACCESS_KEY".to_string(),
                        value: chart_config_prerequisites.aws_secret_access_key.clone(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.PLECO_IDENTIFIER".to_string(),
                        value: chart_config_prerequisites.cluster_id.clone(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.LOG_LEVEL".to_string(),
                        value: "debug".to_string(),
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        }),
    };

    let cluster_agent_context = ClusterAgentContext {
        version: qovery_api
            .service_version(EngineServiceType::ClusterAgent)
            .map_err(|e| {
                CommandError::new("cannot get cluster agent version".to_string(), Some(e.to_string()), None)
            })?,
        api_url: &chart_config_prerequisites.infra_options.qovery_api_url,
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
        version: qovery_api
            .service_version(EngineServiceType::ShellAgent)
            .map_err(|e| CommandError::new("cannot get shell agent version".to_string(), Some(e.to_string()), None))?,
        api_url: &chart_config_prerequisites.infra_options.qovery_api_url,
        organization_long_id: &chart_config_prerequisites.organization_long_id,
        cluster_id: &chart_config_prerequisites.cluster_id,
        cluster_long_id: &chart_config_prerequisites.cluster_long_id,
        cluster_jwt_token: &chart_config_prerequisites.infra_options.jwt_token,
        grpc_url: &chart_config_prerequisites.infra_options.qovery_grpc_url,
    };
    let shell_agent = get_chart_for_shell_agent(shell_context, chart_path, None)?;

    // TODO: Remove this when all cluster have been updated
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
                    value: chart_config_prerequisites.region.clone(),
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
    let mut level_1: Vec<Box<dyn HelmChart>> = vec![
        Box::new(aws_iam_eks_user_mapper),
        Box::new(q_storage_class),
        Box::new(coredns_config),
        Box::new(aws_ui_view),
    ];

    let mut level_2: Vec<Box<dyn HelmChart>> = vec![];

    let level_3: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager)];

    let mut level_4: Vec<Box<dyn HelmChart>> = vec![Box::new(cluster_autoscaler)];

    if let Some(qovery_webhook) = qovery_cert_manager_webhook {
        level_4.push(Box::new(qovery_webhook));
    }

    let level_5: Vec<Box<dyn HelmChart>> = vec![
        Box::new(metrics_server),
        Box::new(aws_node_term_handler),
        Box::new(external_dns),
    ];

    let mut level_6: Vec<Box<dyn HelmChart>> = vec![Box::new(nginx_ingress)];

    let level_7: Vec<Box<dyn HelmChart>> = vec![
        Box::new(cert_manager_config),
        Box::new(qovery_agent), // TODO: Migrate to the new cluster agent
        Box::new(cluster_agent),
        Box::new(shell_agent),
        Box::new(qovery_engine),
    ];

    // observability
    if let Some(kube_prometheus_stack_chart) = kube_prometheus_stack {
        level_1.push(Box::new(kube_prometheus_stack_chart));
    }
    if let Some(prometheus_adapter_chart) = prometheus_adapter {
        level_2.push(Box::new(prometheus_adapter_chart));
    }
    if let Some(kube_state_metrics_chart) = kube_state_metrics {
        level_2.push(Box::new(kube_state_metrics_chart));
    }
    if let Some(promtail_chart) = promtail {
        level_1.push(Box::new(promtail_chart));
    }
    if let Some(loki_chart) = loki {
        level_2.push(Box::new(loki_chart));
    }
    if let Some(grafana_chart) = grafana {
        level_2.push(Box::new(grafana_chart))
    }

    // pleco
    if let Some(pleco_chart) = pleco {
        level_6.push(Box::new(pleco_chart));
    }

    info!("charts configuration preparation finished");
    Ok(vec![level_1, level_2, level_3, level_4, level_5, level_6, level_7])
}
