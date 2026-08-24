use std::{collections::BTreeMap, path::Path};

use crate::byok_chart_gen::values_dot_yaml::{AzureServices, BuildContainer, QoveryEngine};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    QoverySelfManagedChart, SupportedCharts, chart_dot_yaml,
    values_dot_yaml::{
        AwsServices, CertificateServices, ChartConfig, DnsServices, GcpServices, ImageTag, IngressServices,
        LoggingServices, ObservabilityServices, QoveryClusterAgent, QoveryGlobalConfig, QoveryOperator, QoveryServices,
        QoveryShellAgent, ScalewayServices, ServiceEnabled, ServicesEnabler, ValuesFile,
    },
};

#[derive(Error, Debug)]
pub enum ChartDotYamlError {
    #[error("yaml error: {0}")]
    SerdeYaml(serde_yaml::Error),
    #[error("read file error: {0}")]
    ReadFile(std::io::Error),
    #[error("write file error: {0}")]
    WriteFile(std::io::Error),
}

// https://helm.sh/docs/topics/charts/#the-chartyaml-file
#[derive(Serialize, Deserialize)]
pub struct ChartDotYaml {
    #[serde(rename = "apiVersion")]
    pub api_version: ChartDotYamlApiVersion,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<ChartDotYamlDependencies>>,
    pub r#type: Option<ChartDotYamlType>,
    pub version: String,
    #[serde(rename = "appVersion")]
    pub app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "kubeVersion")]
    pub kube_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl ChartDotYaml {
    pub fn from_model(model: chart_dot_yaml::ChartDotYaml) -> Self {
        Self {
            api_version: ChartDotYamlApiVersion::from_model(model.api_version),
            name: model.name,
            description: model.description,
            dependencies: model
                .dependencies
                .map(|d| d.into_iter().map(ChartDotYamlDependencies::from_model).collect()),
            r#type: model.r#type.map(ChartDotYamlType::from_model),
            version: model.version.to_string(),
            app_version: model.app_version.to_string(),
            kube_version: Some(format!("~{}.0-0", model.kube_version.unwrap_or_default())),
            home: model.home,
            icon: model.icon,
        }
    }

    pub fn from_qovery_self_managed_chart(
        prefix: String,
        qovery_chart: QoverySelfManagedChart,
    ) -> Result<ChartDotYaml, ChartDotYamlError> {
        let mut deps = Vec::new();
        for chart_meta in qovery_chart.charts_source_path {
            let chart_file_path = format!("{prefix}/{}/{}/Chart.yaml", chart_meta.source_path, chart_meta.name);
            println!("for chart.yaml, parsing: {chart_file_path}");
            let f = std::fs::File::open(chart_file_path).map_err(ChartDotYamlError::ReadFile)?;
            let chart_version: ChartDotYaml = serde_yaml::from_reader(f).map_err(ChartDotYamlError::SerdeYaml)?;
            let chart_alias = (chart_version.name != chart_meta.name.to_string()).then(|| chart_meta.name.to_string());

            deps.push(ChartDotYamlDependencies {
                name: chart_version.name,
                alias: chart_alias,
                condition: format!("services.{}.{}.enabled", chart_meta.category, chart_meta.name),
                version: chart_version.version,
                repository: format!("file://charts/{}", chart_meta.name),
            })
        }

        Ok(ChartDotYaml {
            api_version: match qovery_chart.api_version {
                chart_dot_yaml::ChartDotYamlApiVersion::V2 => ChartDotYamlApiVersion::V2,
                chart_dot_yaml::ChartDotYamlApiVersion::V1 => ChartDotYamlApiVersion::V1,
            },
            name: qovery_chart.name,
            description: qovery_chart.description,
            dependencies: match deps.is_empty() {
                true => None,
                false => Some(deps),
            },
            r#type: match qovery_chart.r#type {
                chart_dot_yaml::ChartDotYamlType::Application => Some(ChartDotYamlType::Application),
            },
            version: qovery_chart.version.to_string(),
            app_version: qovery_chart.app_version.to_string(),
            kube_version: qovery_chart.kube_version.map(|x| x.to_string()),
            home: Some(qovery_chart.home.to_string()),
            icon: Some(qovery_chart.icon.to_string()),
        })
    }

    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(&self)
    }

    pub fn save_to_file(&self, destination: &Path) -> Result<(), ChartDotYamlError> {
        let file_destination = format!("{}/Chart.yaml", destination.to_string_lossy());
        let f = std::fs::File::create(Path::new(&file_destination)).map_err(ChartDotYamlError::WriteFile)?;
        serde_yaml::to_writer(f, &self).map_err(ChartDotYamlError::SerdeYaml)?;
        Ok(())
    }
}

impl ValuesFile {
    fn enable_envoy_ingress(&mut self) {
        self.services.ingress.ingress_nginx.enabled = false;
        self.services.ingress.envoy_gateway_crd.enabled = true;
        self.services.ingress.envoy_gateway.enabled = true;
        self.services.ingress.qovery_gateway_class.enabled = true;
        self.services.ingress.qovery_cluster_gateway.enabled = true;

        self.ingress_nginx = None;
        self.envoy_gateway_crd = Some(ChartConfig {
            override_chart: Some(SupportedCharts::EnvoyGatewayCrd.to_string()),
        });
        self.envoy_gateway = Some(ChartConfig {
            override_chart: Some(SupportedCharts::EnvoyGateway.to_string()),
        });
        self.qovery_gateway_class = Some(ChartConfig {
            override_chart: Some(SupportedCharts::QoveryGatewayClass.to_string()),
        });
        self.qovery_cluster_gateway = Some(ChartConfig {
            override_chart: Some(SupportedCharts::QoveryClusterGateway.to_string()),
        });
    }

    pub fn new_minimal() -> ValuesFile {
        ValuesFile {
            services: ServicesEnabler {
                qovery: QoveryServices {
                    qovery_cluster_agent: ServiceEnabled { enabled: true },
                    qovery_shell_agent: ServiceEnabled { enabled: true },
                    qovery_operator: ServiceEnabled { enabled: false },
                    qovery_engine: ServiceEnabled { enabled: false },
                    priority_class: ServiceEnabled { enabled: true },
                },
                ingress: IngressServices {
                    ingress_nginx: ServiceEnabled { enabled: false },
                    envoy_gateway_crd: ServiceEnabled { enabled: false },
                    envoy_gateway: ServiceEnabled { enabled: false },
                    qovery_gateway_class: ServiceEnabled { enabled: false },
                    qovery_cluster_gateway: ServiceEnabled { enabled: false },
                },
                dns: DnsServices {
                    external_dns: ServiceEnabled { enabled: false },
                },
                logging: LoggingServices {
                    loki: ServiceEnabled { enabled: false },
                    alloy: ServiceEnabled { enabled: false },
                },
                certificates: CertificateServices {
                    cert_manager: ServiceEnabled { enabled: false },
                    cert_manager_configs: ServiceEnabled { enabled: false },
                    cert_manager_qovery_webhook: ServiceEnabled { enabled: false },
                },
                observability: ObservabilityServices {
                    metrics_server: Some(ServiceEnabled { enabled: false }),
                },
                scaleway: ScalewayServices {
                    qovery_storage_class: ServiceEnabled { enabled: false },
                },
                azure: AzureServices {
                    qovery_storage_class: ServiceEnabled { enabled: false },
                },
                aws: AwsServices {
                    qovery_storage_class: ServiceEnabled { enabled: false },
                    aws_ebs_csi_driver: ServiceEnabled { enabled: false },
                    aws_load_balancer_controller: ServiceEnabled { enabled: false },
                },
                gcp: GcpServices {
                    qovery_storage_class: ServiceEnabled { enabled: false },
                },
            },
            qovery: QoveryGlobalConfig {
                cluster_id: "&clusterId set-by-customer".to_string(),
                cluster_short_id: "&clusterShortId set-by-customer".to_string(),
                organization_id: "&organizationId set-by-customer".to_string(),
                jwt_token: "&jwtToken set-by-customer".to_string(),
                root_domain: "&rootDomain set-by-customer".to_string(),
                domain: "&domain set-by-customer".to_string(),
                domain_wildcard: "&domainWildcard set-by-customer".to_string(),
                qovery_dns_url: "&qoveryDnsUrl set-by-customer".to_string(),
                agent_gateway_url: "&agentGatewayUrl set-by-customer".to_string(),
                engine_gateway_url: "&engineGatewayUrl set-by-customer".to_string(),
                loki_url: "&lokiUrl set-by-customer".to_string(),
                promtail_loki_url: "&promtailLokiUrl set-by-customer".to_string(),
                acme_email_addr: "&acmeEmailAddr set-by-customer".to_string(),
                external_dns_prefix: "&externalDnsPrefix set-by-customer".to_string(),
                architectures: "&architectures set-by-customer".to_string(),
                shell_agent_version: "&shellAgentVersion set-by-customer".to_string(),
                cluster_agent_version: "&clusterAgentVersion set-by-customer".to_string(),
                operator_version: "&operatorVersion set-by-customer".to_string(),
                engine_version: "&engineVersion set-by-customer".to_string(),
            },
            qovery_cluster_agent: QoveryClusterAgent {
                fullname_override: "qovery-shell-agent".to_string(),
                image: ImageTag {
                    tag: "*clusterAgentVersion".to_string(),
                },
                environment_variables: BTreeMap::from([
                    ("CLUSTER_ID".to_string(), "*clusterId".to_string()),
                    ("CLUSTER_JWT_TOKEN".to_string(), "*jwtToken".to_string()),
                    ("ORGANIZATION_ID".to_string(), "*organizationId".to_string()),
                    ("LOKI_URL".to_string(), "*lokiUrl".to_string()),
                    ("GRPC_SERVER".to_string(), "*agentGatewayUrl".to_string()),
                ]),
                use_self_sign_certificate: true,
            },
            qovery_shell_agent: QoveryShellAgent {
                fullname_override: "qovery-shell-agent".to_string(),
                image: ImageTag {
                    tag: "*shellAgentVersion".to_string(),
                },
                environment_variables: BTreeMap::from([
                    ("CLUSTER_ID".to_string(), "*clusterId".to_string()),
                    ("CLUSTER_JWT_TOKEN".to_string(), "*jwtToken".to_string()),
                    ("ORGANIZATION_ID".to_string(), "*organizationId".to_string()),
                    ("GRPC_SERVER".to_string(), "*agentGatewayUrl".to_string()),
                ]),
            },
            qovery_operator: QoveryOperator {
                fullname_override: "qovery-operator".to_string(),
                image: ImageTag {
                    tag: "*operatorVersion".to_string(),
                },
                environment_variables: BTreeMap::from([
                    ("CLUSTER_ID".to_string(), "*clusterId".to_string()),
                    ("CLUSTER_JWT_TOKEN".to_string(), "*jwtToken".to_string()),
                    ("ORGANIZATION_ID".to_string(), "*organizationId".to_string()),
                    ("GRPC_SERVER".to_string(), "*engineGatewayUrl".to_string()),
                ]),
            },
            qovery_engine: Some(QoveryEngine {
                image: ImageTag {
                    tag: "*engineVersion".to_string(),
                },
                engine_resources: None,
                build_container: BuildContainer {
                    environment_variables: BTreeMap::from([
                        ("BUILDER_CPU_ARCHITECTURES".to_string(), "*architectures".to_string()),
                        ("BUILDER_ROOTLESS_ENABLED".to_string(), "true".to_string()),
                    ]),
                },
                environment_variables: BTreeMap::from([
                    ("CLUSTER_ID".to_string(), "*clusterId".to_string()),
                    ("CLUSTER_JWT_TOKEN".to_string(), "*jwtToken".to_string()),
                    ("ORGANIZATION_ID".to_string(), "*organizationId".to_string()),
                    ("DOCKER_HOST".to_string(), "tcp://0.0.0.0:2375".to_string()),
                    ("GRPC_SERVER".to_string(), "*engineGatewayUrl".to_string()),
                    ("LIB_ROOT_DIR".to_string(), "/home/qovery/lib".to_string()),
                ]),
            }),
            ingress_nginx: Some(ChartConfig { override_chart: None }),
            envoy_gateway_crd: None,
            envoy_gateway: None,
            qovery_gateway_class: None,
            qovery_cluster_gateway: None,
            aws_load_balancer_controller: None,
            external_dns: ChartConfig { override_chart: None },
            alloy: ChartConfig { override_chart: None },
            loki: ChartConfig { override_chart: None },
            cert_manager: ChartConfig { override_chart: None },
            cert_manager_qovery_webhook: ChartConfig { override_chart: None },
            cert_manager_configs: ChartConfig { override_chart: None },
            qovery_storage_class_aws: None,
            qovery_storage_class_gcp: None,
            qovery_storage_class_scaleway: None,
            qovery_storage_class_azure: None,
            metrics_server: Some(ChartConfig { override_chart: None }),
        }
    }

    pub fn new_aws() -> ValuesFile {
        let mut value = Self::new_minimal();

        value.enable_envoy_ingress();

        value.services.dns.external_dns.enabled = true;
        value.external_dns.override_chart = Some(SupportedCharts::ExternalDNS.to_string());

        value.services.logging.alloy.enabled = true;
        value.alloy.override_chart = Some(SupportedCharts::Alloy.to_string());
        value.services.logging.loki.enabled = true;
        value.loki.override_chart = Some(SupportedCharts::Loki.to_string());

        value.services.certificates.cert_manager.enabled = true;
        value.cert_manager.override_chart = Some(SupportedCharts::CertManager.to_string());

        value.services.certificates.cert_manager_qovery_webhook.enabled = true;
        value.cert_manager_qovery_webhook.override_chart = Some(SupportedCharts::CertManagerQoveryWebhook.to_string());

        value.services.certificates.cert_manager_configs.enabled = true;
        value.cert_manager_configs.override_chart = Some(SupportedCharts::CertManagerConfigs.to_string());

        value.services.observability.metrics_server = Some(ServiceEnabled { enabled: true });
        value.metrics_server = Some(ChartConfig {
            override_chart: Some(SupportedCharts::MetricsServer.to_string()),
        });

        value.services.aws = AwsServices {
            qovery_storage_class: ServiceEnabled { enabled: true },
            aws_ebs_csi_driver: ServiceEnabled { enabled: false },
            aws_load_balancer_controller: ServiceEnabled { enabled: false },
        };
        value.aws_load_balancer_controller = None;

        value.services.scaleway = ScalewayServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };
        value.services.azure = AzureServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };

        value.services.gcp = GcpServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };
        value.services.qovery.qovery_engine = ServiceEnabled { enabled: true };

        value
    }

    pub fn new_gcp() -> ValuesFile {
        let mut value = Self::new_minimal();

        value.enable_envoy_ingress();

        value.services.dns.external_dns.enabled = true;
        value.external_dns.override_chart = Some(SupportedCharts::ExternalDNS.to_string());

        value.services.logging.alloy.enabled = true;
        value.alloy.override_chart = Some(SupportedCharts::Alloy.to_string());
        value.services.logging.loki.enabled = true;
        value.loki.override_chart = Some(SupportedCharts::Loki.to_string());

        value.services.certificates.cert_manager.enabled = true;
        value.cert_manager.override_chart = Some(SupportedCharts::CertManager.to_string());

        value.services.certificates.cert_manager_qovery_webhook.enabled = true;
        value.cert_manager_qovery_webhook.override_chart = Some(SupportedCharts::CertManagerQoveryWebhook.to_string());

        value.services.certificates.cert_manager_configs.enabled = true;
        value.cert_manager_configs.override_chart = Some(SupportedCharts::CertManagerConfigs.to_string());

        value.services.observability.metrics_server = None;
        value.metrics_server = None;

        value.services.aws = AwsServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
            aws_ebs_csi_driver: ServiceEnabled { enabled: false },
            aws_load_balancer_controller: ServiceEnabled { enabled: false },
        };

        value.services.scaleway = ScalewayServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };
        value.services.azure = AzureServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };
        value.services.gcp = GcpServices {
            qovery_storage_class: ServiceEnabled { enabled: true },
        };

        value.services.qovery.qovery_engine = ServiceEnabled { enabled: true };
        value
    }

    pub fn new_scaleway() -> ValuesFile {
        let mut value = Self::new_minimal();

        value.enable_envoy_ingress();

        value.services.dns.external_dns.enabled = true;
        value.external_dns.override_chart = Some(SupportedCharts::ExternalDNS.to_string());

        value.services.logging.alloy.enabled = true;
        value.alloy.override_chart = Some(SupportedCharts::Alloy.to_string());
        value.services.logging.loki.enabled = true;
        value.loki.override_chart = Some(SupportedCharts::Loki.to_string());

        value.services.certificates.cert_manager.enabled = true;
        value.cert_manager.override_chart = Some(SupportedCharts::CertManager.to_string());

        value.services.certificates.cert_manager_qovery_webhook.enabled = true;
        value.cert_manager_qovery_webhook.override_chart = Some(SupportedCharts::CertManagerQoveryWebhook.to_string());

        value.services.certificates.cert_manager_configs.enabled = true;
        value.cert_manager_configs.override_chart = Some(SupportedCharts::CertManagerConfigs.to_string());

        value.services.observability.metrics_server = None;
        value.metrics_server = None;

        value.services.scaleway = ScalewayServices {
            qovery_storage_class: ServiceEnabled { enabled: true },
        };
        value.services.azure = AzureServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };
        value.services.aws = AwsServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
            aws_ebs_csi_driver: ServiceEnabled { enabled: false },
            aws_load_balancer_controller: ServiceEnabled { enabled: false },
        };

        value.services.gcp = GcpServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };

        value.services.qovery.qovery_engine = ServiceEnabled { enabled: true };
        value
    }

    pub fn new_azure() -> ValuesFile {
        let mut value = Self::new_minimal();

        value.enable_envoy_ingress();

        value.services.dns.external_dns.enabled = true;
        value.external_dns.override_chart = Some(SupportedCharts::ExternalDNS.to_string());

        value.services.logging.alloy.enabled = true;
        value.alloy.override_chart = Some(SupportedCharts::Alloy.to_string());
        value.services.logging.loki.enabled = true;
        value.loki.override_chart = Some(SupportedCharts::Loki.to_string());

        value.services.certificates.cert_manager.enabled = true;
        value.cert_manager.override_chart = Some(SupportedCharts::CertManager.to_string());

        value.services.certificates.cert_manager_qovery_webhook.enabled = true;
        value.cert_manager_qovery_webhook.override_chart = Some(SupportedCharts::CertManagerQoveryWebhook.to_string());

        value.services.certificates.cert_manager_configs.enabled = true;
        value.cert_manager_configs.override_chart = Some(SupportedCharts::CertManagerConfigs.to_string());

        value.services.observability.metrics_server = None;
        value.metrics_server = None;

        value.services.azure = AzureServices {
            qovery_storage_class: ServiceEnabled { enabled: true },
        };
        value.services.scaleway = ScalewayServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };
        value.services.aws = AwsServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
            aws_ebs_csi_driver: ServiceEnabled { enabled: false },
            aws_load_balancer_controller: ServiceEnabled { enabled: false },
        };
        value.services.gcp = GcpServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };

        value.services.qovery.qovery_engine = ServiceEnabled { enabled: true };
        value
    }

    pub fn new_local() -> ValuesFile {
        let mut value = Self::new_minimal();

        value.services.ingress.ingress_nginx.enabled = true;
        value.ingress_nginx = Some(ChartConfig {
            override_chart: Some(SupportedCharts::IngressNginx.to_string()),
        });

        value.services.dns.external_dns.enabled = true;
        value.external_dns.override_chart = Some(SupportedCharts::ExternalDNS.to_string());

        value.services.logging.alloy.enabled = false;
        value.alloy.override_chart = None;
        value.services.logging.loki.enabled = false;
        value.loki.override_chart = None;

        value.services.qovery.qovery_engine = ServiceEnabled { enabled: true };
        value.services.certificates.cert_manager.enabled = true;
        value.cert_manager.override_chart = Some(SupportedCharts::CertManager.to_string());

        value.services.certificates.cert_manager_qovery_webhook.enabled = true;
        value.cert_manager_qovery_webhook.override_chart = Some(SupportedCharts::CertManagerQoveryWebhook.to_string());

        value.services.certificates.cert_manager_configs.enabled = true;
        value.cert_manager_configs.override_chart = Some(SupportedCharts::CertManagerConfigs.to_string());

        value.services.observability.metrics_server = Some(ServiceEnabled { enabled: false });
        value.metrics_server = None;

        value.services.scaleway = ScalewayServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };
        value.services.aws = AwsServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
            aws_ebs_csi_driver: ServiceEnabled { enabled: false },
            aws_load_balancer_controller: ServiceEnabled { enabled: false },
        };
        value.services.azure = AzureServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };
        value.services.gcp = GcpServices {
            qovery_storage_class: ServiceEnabled { enabled: false },
        };

        value
            .qovery_cluster_agent
            .environment_variables
            .insert("LOKI_URL".to_string(), "".to_string());

        value.qovery_engine = Some(QoveryEngine {
            image: ImageTag {
                tag: "*engineVersion".to_string(),
            },
            engine_resources: None,
            build_container: BuildContainer {
                environment_variables: BTreeMap::from([
                    ("BUILDER_CPU_ARCHITECTURES".to_string(), "*architectures".to_string()),
                    ("BUILDER_ROOTLESS_ENABLED".to_string(), "false".to_string()), // need to disable rootless to use http registries
                ]),
            },
            environment_variables: BTreeMap::from([
                ("CLUSTER_ID".to_string(), "*clusterId".to_string()),
                ("CLUSTER_JWT_TOKEN".to_string(), "*jwtToken".to_string()),
                ("ORGANIZATION_ID".to_string(), "*organizationId".to_string()),
                ("DOCKER_HOST".to_string(), "tcp://0.0.0.0:2375".to_string()),
                ("GRPC_SERVER".to_string(), "*engineGatewayUrl".to_string()),
                ("LIB_ROOT_DIR".to_string(), "/home/qovery/lib".to_string()),
            ]),
        });

        value
    }

    pub fn new_demo_local() -> ValuesFile {
        let mut value = Self::new_local();
        value.enable_envoy_ingress();
        value
    }

    pub fn save_to_file(&self, destination: &Path, filename: String) -> Result<(), ChartDotYamlError> {
        let file_destination = format!("{}/{filename}", destination.to_string_lossy());
        let f = std::fs::File::create(Path::new(&file_destination)).map_err(ChartDotYamlError::WriteFile)?;
        serde_yaml::to_writer(f, &self).map_err(ChartDotYamlError::SerdeYaml)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::byok_chart_gen::values_dot_yaml::ValuesFile;

    #[test]
    fn minimal_values_disable_qovery_operator_by_default() {
        let values = ValuesFile::new_minimal();

        assert!(!values.services.qovery.qovery_operator.enabled);
    }

    #[test]
    fn byok_values_enable_envoy_and_disable_nginx() {
        for values in [
            ValuesFile::new_aws(),
            ValuesFile::new_gcp(),
            ValuesFile::new_scaleway(),
            ValuesFile::new_azure(),
            ValuesFile::new_demo_local(),
        ] {
            assert!(!values.services.ingress.ingress_nginx.enabled);
            assert!(values.services.ingress.envoy_gateway_crd.enabled);
            assert!(values.services.ingress.envoy_gateway.enabled);
            assert!(values.services.ingress.qovery_gateway_class.enabled);
            assert!(values.services.ingress.qovery_cluster_gateway.enabled);
            assert!(values.ingress_nginx.is_none());
            assert_eq!(
                values
                    .envoy_gateway_crd
                    .as_ref()
                    .and_then(|config| config.override_chart.as_deref()),
                Some("envoy-gateway-crd")
            );
            assert_eq!(
                values
                    .envoy_gateway
                    .as_ref()
                    .and_then(|config| config.override_chart.as_deref()),
                Some("envoy-gateway")
            );
            assert_eq!(
                values
                    .qovery_cluster_gateway
                    .as_ref()
                    .and_then(|config| config.override_chart.as_deref()),
                Some("qovery-cluster-gateway")
            );
        }
    }

    #[test]
    fn envoy_gateway_overrides_disable_bundled_crds() {
        for override_values in [
            include_str!("../../lib/common/bootstrap/chart_values/envoy-gateway.yaml"),
            include_str!("../../lib/self-managed/demo_chart_values/envoy-gateway.yaml"),
        ] {
            let values: serde_yaml::Value = serde_yaml::from_str(override_values).expect("valid Envoy values");

            assert_eq!(values["crds"]["enabled"], false);
        }
    }

    #[test]
    fn envoy_gateway_overrides_set_qovery_controller_name() {
        for override_values in [
            include_str!("../../lib/common/bootstrap/chart_values/envoy-gateway.yaml"),
            include_str!("../../lib/self-managed/demo_chart_values/envoy-gateway.yaml"),
        ] {
            let values: serde_yaml::Value = serde_yaml::from_str(override_values).expect("valid Envoy values");

            assert_eq!(
                values["config"]["envoyGateway"]["gateway"]["controllerName"],
                "qovery.com/gateway-controller"
            );
        }
    }

    #[test]
    fn dedicated_envoy_gateway_crd_override_enables_gateway_api_and_envoy_crds() {
        let values: serde_yaml::Value =
            serde_yaml::from_str(include_str!("../../lib/common/bootstrap/chart_values/envoy-gateway-crd.yaml"))
                .expect("valid Envoy Gateway CRD values");

        assert_eq!(values["crds"]["gatewayAPI"]["enabled"], true);
        assert_eq!(values["crds"]["envoyGateway"]["enabled"], true);
    }

    #[test]
    fn demo_external_dns_watches_gateway_api_resources_without_tls_route() {
        assert_external_dns_gateway_api_sources_without_tls_route(include_str!(
            "../../lib/self-managed/demo_chart_values/external-dns.yaml"
        ));
    }

    #[test]
    fn byok_external_dns_watches_gateway_api_resources_without_tls_route() {
        assert_external_dns_gateway_api_sources_without_tls_route(include_str!(
            "../../lib/self-managed/chart_values/external-dns.yaml"
        ));
    }

    #[test]
    fn gcp_external_dns_watches_gateway_api_resources_without_tls_route() {
        for values in [
            include_str!("../../lib/gcp/bootstrap/chart_values/external-dns.yaml"),
            include_str!("../../lib/gcp/bootstrap/demo_chart_values/external-dns.yaml"),
        ] {
            assert_external_dns_gateway_api_sources_without_tls_route(values);
        }
    }

    fn assert_external_dns_gateway_api_sources_without_tls_route(values: &str) {
        for source in ["service", "ingress", "gateway-httproute", "gateway-grpcroute"] {
            assert!(values.contains(&format!("  - {source}\n")));
        }
        assert!(!values.contains("  - gateway-tcproute\n"));
        assert!(!values.contains("  - gateway-udproute\n"));
        assert!(!values.contains("  - gateway-tlsroute\n"));
        assert!(values.contains("enableGatewayListenerSets: true"));
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartDotYamlApiVersion {
    #[serde(alias = "V1", alias = "v1")]
    V1,
    #[serde(alias = "V2", alias = "v2")]
    V2,
}

impl ChartDotYamlApiVersion {
    pub fn from_model(model: chart_dot_yaml::ChartDotYamlApiVersion) -> Self {
        match model {
            chart_dot_yaml::ChartDotYamlApiVersion::V1 => ChartDotYamlApiVersion::V1,
            chart_dot_yaml::ChartDotYamlApiVersion::V2 => ChartDotYamlApiVersion::V2,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct ChartDotYamlDependencies {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default)]
    pub condition: String,
    pub version: String,
    pub repository: String,
}

impl ChartDotYamlDependencies {
    pub fn from_model(model: chart_dot_yaml::ChartDotYamlDependencies) -> Self {
        Self {
            name: model.name,
            alias: model.alias,
            condition: model.condition,
            version: model.version.to_string(),
            repository: model.repository,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ChartDotYamlType {
    #[serde(alias = "Application", alias = "application", rename = "application")]
    Application,
}

impl ChartDotYamlType {
    pub fn from_model(model: chart_dot_yaml::ChartDotYamlType) -> Self {
        match model {
            chart_dot_yaml::ChartDotYamlType::Application => ChartDotYamlType::Application,
        }
    }
}
