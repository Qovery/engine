use crate::cloud_provider::aws::kubernetes::{Options, VpcQoveryNetworkMode};
use crate::cloud_provider::helm::{
    get_engine_helm_action_from_location, ChartInfo, ChartSetValue, CommonChart, HelmChart, HelmChartNamespaces,
    UpdateStrategy,
};
use crate::cloud_provider::helm_charts::coredns_config_chart::CoreDNSConfigChart;
use crate::cloud_provider::helm_charts::nginx_ingress_chart::NginxIngressChart;
use crate::cloud_provider::helm_charts::qovery_shell_agent_chart::QoveryShellAgentChart;
use crate::cloud_provider::helm_charts::qovery_storage_class_chart::{QoveryStorageClassChart, QoveryStorageType};
use crate::cloud_provider::helm_charts::{HelmChartResources, HelmChartResourcesConstraintType, ToCommonHelmChart};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cmd::terraform::TerraformError;
use crate::dns_provider::DnsProviderConfiguration;
use crate::errors::CommandError;

use crate::cloud_provider::helm_charts::cert_manager_chart::CertManagerChart;
use crate::cloud_provider::helm_charts::cert_manager_config_chart::CertManagerConfigsChart;
use crate::cloud_provider::helm_charts::external_dns_chart::ExternalDNSChart;
use crate::cloud_provider::helm_charts::metrics_server_chart::MetricsServerChart;
use crate::cloud_provider::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
use crate::cloud_provider::helm_charts::qovery_cluster_agent_chart::QoveryClusterAgentChart;
use crate::cloud_provider::models::{
    CpuArchitecture, CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit,
};
use crate::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::QoveryIdentifier;
use crate::models::third_parties::LetsEncryptConfig;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::iter::FromIterator;
use std::path::Path;
use std::sync::Arc;
use url::Url;

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
    pub cpu_architectures: CpuArchitecture,
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
    pub managed_dns_root_domain_helm_format: String,
    pub external_dns_provider: String,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub dns_provider_config: DnsProviderConfiguration,
    pub disable_pleco: bool,
    // qovery options form json input
    pub infra_options: Options,
}

pub fn get_aws_ec2_qovery_terraform_config(
    qovery_terraform_config_file: &str,
) -> Result<AwsEc2QoveryTerraformConfig, TerraformError> {
    let content_file = match File::open(qovery_terraform_config_file) {
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
    qovery_api: &dyn QoveryApi,
    customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, CommandError> {
    let get_chart_overrride_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> =
        Arc::new(move |chart_name: String| -> Option<CustomerHelmChartsOverride> {
            match customer_helm_charts_override.clone() {
                Some(x) => x.get(&chart_name).map(|content| CustomerHelmChartsOverride {
                    chart_name: chart_name.to_string(),
                    chart_values: content.clone(),
                }),
                None => None,
            }
        });
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
            reinstall_chart_if_installed_version_is_below_than: Some(Version::new(2, 17, 2)),
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
    .to_common_helm_chart()?;

    // CoreDNS config
    let coredns_config = CoreDNSConfigChart::new(
        chart_prefix_path,
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
                // fork to support ARM64 https://github.com/Qovery/registry-creds
                ChartSetValue {
                    key: "image.name".to_string(),
                    value: "public.ecr.aws/r3m4q3r9/registry-creds".to_string(),
                },
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: "2023-08-16T09-23-02".to_string(),
                },
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

    // External DNS
    let external_dns = ExternalDNSChart::new(
        chart_prefix_path,
        chart_config_prerequisites.dns_provider_config.clone(),
        chart_config_prerequisites
            .managed_dns_root_domain_helm_format
            .to_string(),
        false,
        chart_config_prerequisites.cluster_id.to_string(),
        UpdateStrategy::Recreate,
        false,
    )
    .to_common_helm_chart()?;

    let mut qovery_cert_manager_webhook: Option<CommonChart> = None;
    if let DnsProviderConfiguration::QoveryDns(qovery_dns_config) = &chart_config_prerequisites.dns_provider_config {
        qovery_cert_manager_webhook = Some(
            QoveryCertManagerWebhookChart::new(
                chart_prefix_path,
                qovery_dns_config.clone(),
                HelmChartResourcesConstraintType::Constrained(HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(32),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(32),
                }),
                UpdateStrategy::Recreate,
            )
            .to_common_helm_chart()?,
        );
    }

    // Metrics server
    let metrics_server = MetricsServerChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::Constrained(HelmChartResources {
            limit_cpu: KubernetesCpuResourceUnit::MilliCpu(250),
            limit_memory: KubernetesMemoryResourceUnit::MebiByte(30),
            request_cpu: KubernetesCpuResourceUnit::MilliCpu(250),
            request_memory: KubernetesMemoryResourceUnit::MebiByte(30),
        }),
        UpdateStrategy::Recreate,
        false,
    )
    .to_common_helm_chart()?;

    // Cert Manager chart
    let cert_manager = CertManagerChart::new(
        chart_prefix_path,
        false, // Due to cycle, prometheus need tls certificate from cert manager, and enabling this will require prometheus to be already installed
        HelmChartResourcesConstraintType::Constrained(HelmChartResources {
            limit_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
            limit_memory: KubernetesMemoryResourceUnit::MebiByte(96),
            request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
            request_memory: KubernetesMemoryResourceUnit::MebiByte(96),
        }),
        HelmChartResourcesConstraintType::Constrained(HelmChartResources {
            request_cpu: KubernetesCpuResourceUnit::MilliCpu(50),
            request_memory: KubernetesMemoryResourceUnit::MebiByte(64),
            limit_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
            limit_memory: KubernetesMemoryResourceUnit::MebiByte(64),
        }),
        HelmChartResourcesConstraintType::Constrained(HelmChartResources {
            request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
            request_memory: KubernetesMemoryResourceUnit::MebiByte(96),
            limit_cpu: KubernetesCpuResourceUnit::MilliCpu(500),
            limit_memory: KubernetesMemoryResourceUnit::MebiByte(96),
        }),
        UpdateStrategy::Recreate,
        get_chart_overrride_fn.clone(),
        false,
    )
    .to_common_helm_chart()?;

    // Cert Manager Configs
    let cert_manager_config = CertManagerConfigsChart::new(
        chart_prefix_path,
        &chart_config_prerequisites.lets_encrypt_config,
        &chart_config_prerequisites.dns_provider_config,
        chart_config_prerequisites.managed_dns_helm_format.to_string(),
    )
    .to_common_helm_chart()?;

    // Nginx ingress
    let nginx_ingress = NginxIngressChart::new(
        chart_prefix_path,
        HelmChartResourcesConstraintType::Constrained(HelmChartResources {
            limit_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
            limit_memory: KubernetesMemoryResourceUnit::MebiByte(256),
            request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
            request_memory: KubernetesMemoryResourceUnit::MebiByte(256),
        }),
        HelmChartResourcesConstraintType::ChartDefault,
        false, // no metrics history on EC2 ATM
        get_chart_overrride_fn.clone(),
    )
    .to_common_helm_chart()?;

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
        &chart_config_prerequisites.infra_options.jwt_token.clone(),
        QoveryIdentifier::new(chart_config_prerequisites.cluster_long_id),
        QoveryIdentifier::new(chart_config_prerequisites.organization_long_id),
        HelmChartResourcesConstraintType::Constrained(HelmChartResources {
            limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
            limit_memory: KubernetesMemoryResourceUnit::MebiByte(100),
            request_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
            request_memory: KubernetesMemoryResourceUnit::MebiByte(50),
        }),
        UpdateStrategy::Recreate,
        false,
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
        HelmChartResourcesConstraintType::Constrained(HelmChartResources {
            limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
            limit_memory: KubernetesMemoryResourceUnit::MebiByte(100),
            request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
            request_memory: KubernetesMemoryResourceUnit::MebiByte(50),
        }),
        UpdateStrategy::Recreate,
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
                // autoscaler
                ChartSetValue {
                    key: "autoscaler.enabled".to_string(),
                    value: "false".to_string(),
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
                // builder
                ChartSetValue {
                    key: "buildContainer.enabled".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "buildContainer.environmentVariables.BUILDER_CPU_ARCHITECTURES".to_string(),
                    value: chart_config_prerequisites.cpu_architectures.to_string(),
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
                    value: "1Gi".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.requests.memory".to_string(),
                    value: "1Gi".to_string(),
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };

    // deploy sequentially to avoid insufficient resources
    // chart deployment order matters!!!
    let mut prepare_chats_to_deploy: Vec<Box<dyn HelmChart>> = vec![
        Box::new(aws_ebs_csi_driver_secret),
        Box::new(aws_ebs_csi_driver),
        Box::new(q_storage_class),
        Box::new(coredns_config),
        Box::new(registry_creds),
        Box::new(cert_manager),
    ];

    if let Some(qovery_webhook) = qovery_cert_manager_webhook {
        prepare_chats_to_deploy.push(Box::new(qovery_webhook));
    };

    prepare_chats_to_deploy.append(&mut vec![
        Box::new(external_dns),
        Box::new(metrics_server),
        Box::new(nginx_ingress),
        Box::new(nginx_ingress_wildcard_dns_record),
        Box::new(cert_manager_config),
        Box::new(qovery_engine),
        Box::new(qovery_cluster_agent),
        Box::new(qovery_shell_agent),
    ]);

    info!("charts configuration preparation finished");
    let mut charts_to_deploy = Vec::with_capacity(prepare_chats_to_deploy.len());
    for chart in prepare_chats_to_deploy {
        charts_to_deploy.push(vec![chart]);
    }
    Ok(charts_to_deploy)
}
