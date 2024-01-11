use std::{collections::BTreeMap, path::Path};

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    chart_dot_yaml,
    values_dot_yaml::{
        CertificateServices, ChartConfig, DnsServices, ImageTag, IngressServices, LoggingServices,
        ObservabilityServices, QoveryAgents, QoveryGlobalConfig, QoveryServices, ServiceEnabled, ServicesEnabler,
        ValuesFile,
    },
    QoverySelfManagedChart, SupportedCharts,
};

#[derive(Error, Debug)]
pub enum ChartDotYamlError {
    #[error("semver version not valid: {0}")]
    SemVerParseError(semver::Error),
    #[error("yaml error: {0}")]
    SerdeYamlError(serde_yaml::Error),
    #[error("read file error: {0}")]
    ReadFileError(std::io::Error),
    #[error("write file error: {0}")]
    WriteFileError(std::io::Error),
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
    pub fn to_model(&self) -> Result<chart_dot_yaml::ChartDotYaml, ChartDotYamlError> {
        let dependencies = match self.dependencies.as_ref() {
            Some(x) => {
                let mut deps = Vec::with_capacity(x.len());
                for dep in x {
                    deps.push(dep.to_model()?);
                }
                Some(deps)
            }
            None => None,
        };

        Ok(chart_dot_yaml::ChartDotYaml {
            api_version: self.api_version.to_model(),
            name: self.name.clone(),
            description: self.description.clone(),
            dependencies,
            r#type: self.r#type.clone().map(|t| t.to_model()),
            version: Version::parse(self.version.as_str()).map_err(ChartDotYamlError::SemVerParseError)?,
            app_version: Version::parse(self.app_version.as_str()).map_err(ChartDotYamlError::SemVerParseError)?,
            kube_version: self.kube_version.clone(),
            home: self.home.clone(),
            icon: self.icon.clone(),
        })
    }

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
            let f = std::fs::File::open(chart_file_path).map_err(ChartDotYamlError::ReadFileError)?;
            let chart_version: ChartDotYaml = serde_yaml::from_reader(f).map_err(ChartDotYamlError::SerdeYamlError)?;

            deps.push(ChartDotYamlDependencies {
                name: chart_meta.name.to_string(),
                alias: None,
                condition: format!("{}.{}.enabled", chart_meta.category, chart_meta.name),
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
            kube_version: Some(qovery_chart.kube_version.to_string()),
            home: Some(qovery_chart.home.to_string()),
            icon: Some(qovery_chart.icon.to_string()),
        })
    }

    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(&self)
    }

    pub fn save_to_file(&self, destination: &Path) -> Result<(), ChartDotYamlError> {
        let file_destination = format!("{}/Chart.yaml", destination.to_string_lossy());
        let f = std::fs::File::create(Path::new(&file_destination)).map_err(ChartDotYamlError::WriteFileError)?;
        serde_yaml::to_writer(f, &self).map_err(ChartDotYamlError::SerdeYamlError)?;
        Ok(())
    }
}

impl ValuesFile {
    pub fn new_minimal() -> ValuesFile {
        ValuesFile {
            services: ServicesEnabler {
                qovery: QoveryServices {
                    qovery_cluster_agent: ServiceEnabled { enabled: true },
                    qovery_shell_agent: ServiceEnabled { enabled: true },
                    qovery_engine: ServiceEnabled { enabled: false },
                },
                ingress: IngressServices {
                    ingress_nginx: ServiceEnabled { enabled: false },
                },
                dns: DnsServices {
                    external_dns: ServiceEnabled { enabled: false },
                },
                logging: LoggingServices {
                    loki: ServiceEnabled { enabled: false },
                    promtail: ServiceEnabled { enabled: false },
                },
                certificates: CertificateServices {
                    cert_manager: ServiceEnabled { enabled: false },
                    cert_manager_configs: ServiceEnabled { enabled: false },
                    cert_manager_qovery_webhook: ServiceEnabled { enabled: false },
                },
                observability: ObservabilityServices {
                    metrics_server: Some(ServiceEnabled { enabled: false }),
                },
                aws: None,
                gcp: None,
                scaleway: None,
            },
            qovery: QoveryGlobalConfig {
                cluster_id: "&clusterId set-by-customer".to_string(),
                cluster_short_id: "&clusterShortId set-by-customer".to_string(),
                organization_id: "&organizationId set-by-customer".to_string(),
                jwt_token: "&jwtToken set-by-customer".to_string(),
                domain: "&domain set-by-customer".to_string(),
                grpc_server: "&grpcServer set-by-customer".to_string(),
                engine_grpc_server: "&engineGrpcServer set-by-customer".to_string(),
                qovery_dns_url: "&qoveryDnsUrl set-by-customer".to_string(),
                loki_url: "&lokiUrl set-by-customer".to_string(),
                promtail_loki_url: "&promtailLokiUrl set-by-customer".to_string(),
                acme_email_addr: "&acmeEmailAddr set-by-customer".to_string(),
                external_dns_prefix: "&externalDnsPrefix set-by-customer".to_string(),
                architectures: "&architectures set-by-customer".to_string(),
            },
            qovery_cluster_agent: QoveryAgents {
                full_name_override: "qovery-shell-agent".to_string(),
                image: ImageTag {
                    tag: "latest".to_string(),
                },
                environment_variables: BTreeMap::from([
                    ("CLUSTER_ID".to_string(), "*clusterId".to_string()),
                    ("CLUSTER_JWT_TOKEN".to_string(), "*jwtToken".to_string()),
                    ("GRPC_SERVER".to_string(), "*grpcServer".to_string()),
                    ("ORGANIZATION_ID".to_string(), "*organizationId".to_string()),
                    ("LOKI_URL".to_string(), "*lokiUrl".to_string()),
                ]),
            },
            qovery_shell_agent: QoveryAgents {
                full_name_override: "qovery-shell-agent".to_string(),
                image: ImageTag {
                    tag: "latest".to_string(),
                },
                environment_variables: BTreeMap::from([
                    ("CLUSTER_ID".to_string(), "*clusterId".to_string()),
                    ("CLUSTER_JWT_TOKEN".to_string(), "*jwtToken".to_string()),
                    ("GRPC_SERVER".to_string(), "*grpcServer".to_string()),
                    ("ORGANIZATION_ID".to_string(), "*organizationId".to_string()),
                ]),
            },
            ingress_nginx: ChartConfig { override_chart: None },
            external_dns: ChartConfig { override_chart: None },
            promtail: ChartConfig { override_chart: None },
            loki: ChartConfig { override_chart: None },
            cert_manager: ChartConfig { override_chart: None },
            cert_manager_qovery_webhook: ChartConfig { override_chart: None },
            cert_manager_configs: ChartConfig { override_chart: None },
            qovery_storage_class: None,
            metrics_server: Some(ChartConfig { override_chart: None }),
        }
    }

    pub fn new_aws() -> ValuesFile {
        let mut value = Self::new_minimal();

        value.services.ingress.ingress_nginx.enabled = true;
        value.ingress_nginx.override_chart = Some(SupportedCharts::IngressNginx.to_string());

        value.services.dns.external_dns.enabled = true;
        value.external_dns.override_chart = Some(SupportedCharts::ExternalDNS.to_string());

        value.services.logging.promtail.enabled = true;
        value.promtail.override_chart = Some(SupportedCharts::Promtail.to_string());
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

        value.services.aws = None;

        value
    }

    pub fn new_gcp() -> ValuesFile {
        let mut value = Self::new_minimal();

        value.services.ingress.ingress_nginx.enabled = true;
        value.ingress_nginx.override_chart = Some(SupportedCharts::IngressNginx.to_string());

        value.services.dns.external_dns.enabled = true;
        value.external_dns.override_chart = Some(SupportedCharts::ExternalDNS.to_string());

        value.services.logging.promtail.enabled = true;
        value.promtail.override_chart = Some(SupportedCharts::Promtail.to_string());
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

        value.qovery_storage_class = None;

        value
    }

    pub fn new_scaleway() -> ValuesFile {
        let mut value = Self::new_minimal();

        value.services.ingress.ingress_nginx.enabled = true;
        value.ingress_nginx.override_chart = Some(SupportedCharts::IngressNginx.to_string());

        value.services.dns.external_dns.enabled = true;
        value.external_dns.override_chart = Some(SupportedCharts::ExternalDNS.to_string());

        value.services.logging.promtail.enabled = true;
        value.promtail.override_chart = Some(SupportedCharts::Promtail.to_string());
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

        value.qovery_storage_class = None;

        value
    }

    pub fn save_to_file(&self, destination: &Path, filename: String) -> Result<(), ChartDotYamlError> {
        let file_destination = format!("{}/{filename}", destination.to_string_lossy());
        let f = std::fs::File::create(Path::new(&file_destination)).map_err(ChartDotYamlError::WriteFileError)?;
        serde_yaml::to_writer(f, &self).map_err(ChartDotYamlError::SerdeYamlError)?;
        Ok(())
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
    pub fn to_model(&self) -> chart_dot_yaml::ChartDotYamlApiVersion {
        match self {
            ChartDotYamlApiVersion::V1 => chart_dot_yaml::ChartDotYamlApiVersion::V1,
            ChartDotYamlApiVersion::V2 => chart_dot_yaml::ChartDotYamlApiVersion::V2,
        }
    }

    pub fn from_model(model: chart_dot_yaml::ChartDotYamlApiVersion) -> Self {
        match model {
            chart_dot_yaml::ChartDotYamlApiVersion::V1 => ChartDotYamlApiVersion::V1,
            chart_dot_yaml::ChartDotYamlApiVersion::V2 => ChartDotYamlApiVersion::V2,
        }
    }
}

#[derive(Serialize, Deserialize)]
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
    pub fn to_model(&self) -> Result<chart_dot_yaml::ChartDotYamlDependencies, ChartDotYamlError> {
        Ok(chart_dot_yaml::ChartDotYamlDependencies {
            name: self.name.clone(),
            alias: self.alias.clone(),
            condition: format!("services.{}", self.condition),
            version: Version::parse(self.version.as_str()).map_err(ChartDotYamlError::SemVerParseError)?,
            repository: self.repository.clone(),
        })
    }

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
    pub fn to_model(&self) -> chart_dot_yaml::ChartDotYamlType {
        match self {
            ChartDotYamlType::Application => chart_dot_yaml::ChartDotYamlType::Application,
        }
    }

    pub fn from_model(model: chart_dot_yaml::ChartDotYamlType) -> Self {
        match model {
            chart_dot_yaml::ChartDotYamlType::Application => ChartDotYamlType::Application,
        }
    }
}
