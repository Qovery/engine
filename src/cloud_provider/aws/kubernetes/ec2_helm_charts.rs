use crate::cloud_provider::aws::kubernetes::{Options, VpcQoveryNetworkMode};
use crate::cloud_provider::helm::{
    get_chart_for_cert_manager_config, get_chart_for_cluster_agent, get_chart_for_shell_agent,
    get_engine_helm_action_from_location, ChartInfo, ChartSetValue, ClusterAgentContext, CommonChart, HelmAction,
    HelmChart, HelmChartNamespaces, ShellAgentContext,
};
use crate::cloud_provider::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::cloud_provider::helm_charts::ToCommonHelmChart;
use crate::cloud_provider::qovery::{get_qovery_app_version, EngineLocation, QoveryAppName, QoveryEngine};
use crate::cmd::terraform::TerraformError;
use crate::dns_provider::DnsProviderConfiguration;
use crate::errors::CommandError;

use crate::cloud_provider::helm_charts::core_dns_config_chart::CoreDNSConfigChart;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::iter::FromIterator;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsEc2QoveryTerraformConfig {
    pub aws_ec2_public_hostname: String,
    pub aws_ec2_kubernetes_port: String,
    pub aws_aws_account_id: String,
}

impl AwsEc2QoveryTerraformConfig {
    pub fn kubernetes_port_to_u16(&self) -> Result<u16, String> {
        match self.aws_ec2_kubernetes_port.parse::<u16>() {
            Ok(x) => Ok(x),
            Err(e) => Err(format!(
                "error while trying to convert kubernetes port from string {} to int: {}",
                self.aws_ec2_kubernetes_port, e
            )),
        }
    }
}

pub struct Ec2ChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub region: String,
    pub cluster_name: String,
    pub cloud_provider: String,
    pub test_cluster: bool,
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    pub vpc_qovery_network_mode: VpcQoveryNetworkMode,
    pub qovery_engine_location: EngineLocation,
    pub ff_log_history_enabled: bool,
    pub ff_metrics_history_enabled: bool,
    pub managed_dns_name: String,
    pub managed_dns_name_wildcarded: String,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub external_dns_provider: String,
    pub dns_email_report: String,
    pub acme_url: String,
    pub dns_provider_config: DnsProviderConfiguration,
    pub disable_pleco: bool,
    // qovery options form json input
    pub infra_options: Options,
}

pub fn get_aws_ec2_qovery_terraform_config(
    qovery_terraform_config_file: &str,
) -> Result<AwsEc2QoveryTerraformConfig, TerraformError> {
    let content_file = match File::open(&qovery_terraform_config_file) {
        Ok(x) => x,
        Err(e) => {
            return Err(TerraformError::ConfigFileNotFound {
                path: qovery_terraform_config_file.to_string(),
                raw_message: e.to_string(),
            });
        }
    };

    let reader = BufReader::new(content_file);
    match serde_json::from_reader(reader) {
        Ok(config) => Ok(config),
        Err(e) => Err(TerraformError::ConfigFileInvalidContent {
            path: qovery_terraform_config_file.to_string(),
            raw_message: e.to_string(),
        }),
    }
}

pub fn ec2_aws_helm_charts(
    qovery_terraform_config_file: &str,
    chart_config_prerequisites: &Ec2ChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    _kubernetes_config: &Path,
    _envs: &[(String, String)],
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
    let chart_prefix = chart_prefix_path.unwrap_or("./");
    let chart_path = |x: &str| -> String { format!("{}/{}", &chart_prefix, x) };
    let qovery_terraform_config = get_aws_ec2_qovery_terraform_config(qovery_terraform_config_file)?;

    // CSI driver
    let aws_ebs_csi_driver_secret = CommonChart {
        chart_info: ChartInfo {
            name: "aws-ebs-csi-driver-secret".to_string(),
            path: chart_path("/charts/aws-ebs-csi-driver-secret"),
            values: vec![
                ChartSetValue {
                    key: "awsAccessKeyId".to_string(),
                    value: chart_config_prerequisites.aws_access_key_id.clone(),
                },
                ChartSetValue {
                    key: "awsSecretAccessKeyId".to_string(),
                    value: chart_config_prerequisites.aws_secret_access_key.clone(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    let aws_ebs_csi_driver = CommonChart {
        chart_info: ChartInfo {
            name: "aws-ebs-csi-driver".to_string(),
            path: chart_path("/charts/aws-ebs-csi-driver"),
            values: vec![ChartSetValue {
                key: "controller.replicaCount".to_string(),
                value: "1".to_string(),
            }],
            ..Default::default()
        },
        ..Default::default()
    };

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

    // CoreDNS config
    let coredns_config = CoreDNSConfigChart::new(
        chart_prefix_path,
        vec![
            "eks.amazonaws.com/component: coredns".to_string(),
            "k8s-app: kube-dns".to_string(),
        ],
        true,
        chart_config_prerequisites.managed_dns_helm_format.to_string(),
        chart_config_prerequisites
            .managed_dns_resolvers_terraform_format
            .to_string(),
    );

    let registry_creds = CommonChart {
        chart_info: ChartInfo {
            name: "registry-creds".to_string(),
            path: chart_path("charts/registry-creds"),
            values: vec![
                ChartSetValue {
                    key: "ecr.enabled".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "ecr.awsAccessKeyId".to_string(),
                    value: chart_config_prerequisites.aws_access_key_id.clone(),
                },
                ChartSetValue {
                    key: "ecr.awsSecretAccessKey".to_string(),
                    value: chart_config_prerequisites.aws_secret_access_key.clone(),
                },
                ChartSetValue {
                    key: "ecr.awsRegion".to_string(),
                    value: chart_config_prerequisites.region.clone(),
                },
            ],
            values_string: vec![ChartSetValue {
                key: "ecr.awsAccount".to_string(),
                value: qovery_terraform_config.aws_aws_account_id,
            }],
            ..Default::default()
        },
        ..Default::default()
    };

    let external_dns = CommonChart {
        chart_info: ChartInfo {
            name: "externaldns".to_string(),
            path: chart_path("common/charts/external-dns"),
            values_files: vec![chart_path("chart_values/external-dns.yaml")],
            values: vec![
                // resources limits
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "30Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "30Mi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

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

    let metrics_server = CommonChart {
        chart_info: ChartInfo {
            name: "metrics-server".to_string(),
            path: chart_path("common/charts/metrics-server"),
            values_files: vec![chart_path("chart_values/metrics-server.yaml")],
            values: vec![
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "30Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "30Mi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    let cert_manager = CommonChart {
        chart_info: ChartInfo {
            name: "cert-manager".to_string(),
            path: chart_path("common/charts/cert-manager"),
            namespace: HelmChartNamespaces::CertManager,
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
                    // Due to cycle, prometheus need tls certificate from cert manager, and enabling this will require
                    // prometheus to be already installed
                    value: "false".to_string(),
                },
                ChartSetValue {
                    key: "prometheus.servicemonitor.prometheusInstance".to_string(),
                    value: "qovery".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "96Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "96Mi".to_string(),
                },
                // Webhooks resources limits
                ChartSetValue {
                    key: "webhook.resources.limits.memory".to_string(),
                    value: "64Mi".to_string(),
                },
                ChartSetValue {
                    key: "webhook.resources.requests.memory".to_string(),
                    value: "64Mi".to_string(),
                },
                // Cainjector resources limits
                ChartSetValue {
                    key: "cainjector.resources.limits.memory".to_string(),
                    value: "96Mi".to_string(),
                },
                ChartSetValue {
                    key: "cainjector.resources.requests.memory".to_string(),
                    value: "96Mi".to_string(),
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
                // Memory is set to 256Mi to prevent random OOM on x64
                ChartSetValue {
                    key: "controller.resources.limits.memory".to_string(),
                    value: "256Mi".to_string(),
                },
                ChartSetValue {
                    key: "controller.resources.requests.memory".to_string(),
                    value: "256Mi".to_string(),
                },
                // Default backend resources limits
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

    let nginx_ingress_wildcard_dns_record = CommonChart {
        chart_info: ChartInfo {
            name: "nginx-ingress-wildcard-dns-record".to_string(),
            path: chart_path("common/charts/external-name-svc"),
            namespace: HelmChartNamespaces::NginxIngress,
            values: vec![
                ChartSetValue {
                    key: "serviceName".to_string(),
                    value: "nginx-ingress-wildcard-dns-record".to_string(),
                },
                ChartSetValue {
                    key: "source".to_string(),
                    value: chart_config_prerequisites.managed_dns_name_wildcarded.to_string(),
                },
                ChartSetValue {
                    key: "destination".to_string(),
                    value: qovery_terraform_config.aws_ec2_public_hostname,
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
    let cluster_agent_resources = vec![
        ChartSetValue {
            key: "resources.requests.memory".to_string(),
            value: "50Mi".to_string(),
        },
        ChartSetValue {
            key: "resources.limits.memory".to_string(),
            value: "100Mi".to_string(),
        },
    ];
    let cluster_agent = get_chart_for_cluster_agent(cluster_agent_context, chart_path, Some(cluster_agent_resources))?;

    let shell_context = ShellAgentContext {
        api_url: &chart_config_prerequisites.infra_options.qovery_api_url,
        api_token: &chart_config_prerequisites.infra_options.agent_version_controller_token,
        organization_long_id: &chart_config_prerequisites.organization_long_id,
        cluster_id: &chart_config_prerequisites.cluster_id,
        cluster_long_id: &chart_config_prerequisites.cluster_long_id,
        cluster_jwt_token: &chart_config_prerequisites.infra_options.jwt_token,
        grpc_url: &chart_config_prerequisites.infra_options.qovery_grpc_url,
    };
    let shell_agent_resources = vec![
        ChartSetValue {
            key: "resources.requests.memory".to_string(),
            value: "50Mi".to_string(),
        },
        ChartSetValue {
            key: "resources.limits.memory".to_string(),
            value: "100Mi".to_string(),
        },
    ];
    let shell_agent = get_chart_for_shell_agent(shell_context, chart_path, Some(shell_agent_resources))?;

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
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "metrics.enabled".to_string(),
                    value: "false".to_string(), // update this field if we decide to add prometheus support later on EC2
                },
                ChartSetValue {
                    key: "volumes.storageClassName".to_string(),
                    value: "aws-ebs-gp2-0".to_string(),
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
    let level_1: Vec<Box<dyn HelmChart>> = vec![
        Box::new(aws_ebs_csi_driver_secret),
        Box::new(aws_ebs_csi_driver),
        Box::new(q_storage_class),
        Box::new(coredns_config),
        Box::new(registry_creds),
    ];

    let level_2: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager)];

    let level_3: Vec<Box<dyn HelmChart>> = if let Some(qovery_webhook) = qovery_cert_manager_webhook {
        vec![Box::new(qovery_webhook)]
    } else {
        vec![]
    };

    let level_4: Vec<Box<dyn HelmChart>> = vec![];

    let level_5: Vec<Box<dyn HelmChart>> = vec![Box::new(external_dns), Box::new(metrics_server)];

    let level_6: Vec<Box<dyn HelmChart>> = vec![Box::new(nginx_ingress)];

    let level_7: Vec<Box<dyn HelmChart>> = vec![
        Box::new(nginx_ingress_wildcard_dns_record),
        Box::new(cert_manager_config),
        Box::new(qovery_agent), // TODO: Migrate to the new cluster agent
        Box::new(qovery_engine),
        Box::new(cluster_agent),
        Box::new(shell_agent),
    ];

    info!("charts configuration preparation finished");
    Ok(vec![level_1, level_2, level_3, level_4, level_5, level_6, level_7])
}
