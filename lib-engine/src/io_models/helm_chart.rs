use crate::engine_task::qovery_api::QoveryApi;
use crate::environment::models;
use crate::environment::models::aws::AwsAppExtraSettings;
use crate::environment::models::azure::AzureAppExtraSettings;
use crate::environment::models::gcp::GcpAppExtraSettings;
use crate::environment::models::helm_chart::{HelmChartError, HelmChartService};
use crate::environment::models::scaleway::ScwAppExtraSettings;
use crate::environment::models::selfmanaged::OnPremiseAppExtraSettings;
use crate::environment::models::types::{AWS, Azure, GCP, OnPremise, SCW};
use crate::infrastructure::models::build_platform::SshKey;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::cloud_provider::io::{NginxConfigurationSnippet, NginxServerSnippet};
use crate::infrastructure::models::cloud_provider::service::ServiceType;
use crate::infrastructure::models::kubernetes;
use crate::io_models::application::{GitCredentials, Port};
use crate::io_models::container::Registry;
use crate::io_models::context::Context;
use crate::io_models::variable_utils::{VariableInfo, default_environment_vars_with_info};
use crate::io_models::{Action, fetch_git_token, ssh_keys_from_env_vars};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct HelmCredentials {
    pub login: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct HelmChartAdvancedSettings {
    // Ingress
    #[serde(alias = "network.ingress.proxy_body_size_mb")]
    pub network_ingress_proxy_body_size_mb: u32,
    #[serde(alias = "network.ingress.force_ssl_redirect")]
    pub network_ingress_force_ssl_redirect: bool,
    #[serde(alias = "network.ingress.enable_cors")]
    pub network_ingress_cors_enable: bool,
    #[serde(alias = "network.ingress.enable_sticky_session")]
    pub network_ingress_sticky_session_enable: bool,
    #[serde(alias = "network.ingress.cors_allow_origin")]
    pub network_ingress_cors_allow_origin: String,
    #[serde(alias = "network.ingress.cors_allow_methods")]
    pub network_ingress_cors_allow_methods: String,
    #[serde(alias = "network.ingress.cors_allow_headers")]
    pub network_ingress_cors_allow_headers: String,
    #[serde(alias = "network.ingress.keepalive_time_seconds")]
    pub network_ingress_keepalive_time_seconds: u32,
    #[serde(alias = "network.ingress.keepalive_timeout_seconds")]
    pub network_ingress_keepalive_timeout_seconds: u32,
    #[serde(alias = "network.ingress.send_timeout_seconds")]
    pub network_ingress_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.add_headers")]
    pub network_ingress_add_headers: BTreeMap<String, String>,
    #[serde(alias = "network.ingress.proxy_set_headers")]
    pub network_ingress_proxy_set_headers: BTreeMap<String, String>,
    #[serde(alias = "network.ingress.proxy_connect_timeout_seconds")]
    pub network_ingress_proxy_connect_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_send_timeout_seconds")]
    pub network_ingress_proxy_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_read_timeout_seconds")]
    pub network_ingress_proxy_read_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_request_buffering")]
    pub network_ingress_proxy_request_buffering: String,
    #[serde(alias = "network.ingress.proxy_buffering")]
    pub network_ingress_proxy_buffering: String,
    #[serde(alias = "network.ingress.proxy_buffer_size_kb")]
    pub network_ingress_proxy_buffer_size_kb: u32,
    #[serde(alias = "network.ingress.whitelist_source_range")]
    pub network_ingress_whitelist_source_range: String,
    #[serde(alias = "network.ingress.denylist_source_range")]
    pub network_ingress_denylist_source_range: String,
    #[serde(alias = "network.ingress.basic_auth_env_var")]
    pub network_ingress_basic_auth_env_var: String,
    #[serde(alias = "network.ingress.nginx_limit_rpm")]
    pub network_ingress_nginx_limit_rpm: Option<u32>,
    #[serde(alias = "network.ingress.nginx_limit_rps")]
    pub network_ingress_nginx_limit_rps: Option<u32>,
    #[serde(alias = "network.ingress.nginx_limit_burst_multiplier")]
    pub network_ingress_nginx_limit_burst_multiplier: Option<u32>,
    #[serde(alias = "network.ingress.nginx_limit_connections")]
    pub network_ingress_nginx_limit_connections: Option<u32>,
    #[serde(alias = "network.ingress.nginx_controller_server_snippet")]
    pub network_ingress_nginx_controller_server_snippet: Option<NginxServerSnippet>,
    #[serde(alias = "network.ingress.nginx_controller_configuration_snippet")]
    pub network_ingress_nginx_controller_configuration_snippet: Option<NginxConfigurationSnippet>,
    #[serde(alias = "network.ingress.nginx_custom_http_errors")]
    pub network_ingress_nginx_custom_http_errors: Option<String>,

    #[serde(alias = "network.ingress.grpc_send_timeout_seconds")]
    pub network_ingress_grpc_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.grpc_read_timeout_seconds")]
    pub network_ingress_grpc_read_timeout_seconds: u32,
}

impl Default for HelmChartAdvancedSettings {
    fn default() -> Self {
        HelmChartAdvancedSettings {
            network_ingress_proxy_body_size_mb: 100,
            network_ingress_force_ssl_redirect: true,
            network_ingress_cors_enable: false,
            network_ingress_sticky_session_enable: false,
            network_ingress_cors_allow_origin: "*".to_string(),
            network_ingress_cors_allow_methods: "GET, PUT, POST, DELETE, PATCH, OPTIONS".to_string(),
            network_ingress_cors_allow_headers: "DNT,Keep-Alive,User-Agent,X-Requested-With,If-Modified-Since,Cache-Control,Content-Type,Range,Authorization".to_string(),
            network_ingress_keepalive_time_seconds: 3600,
            network_ingress_keepalive_timeout_seconds: 60,
            network_ingress_send_timeout_seconds: 60,
            network_ingress_add_headers: BTreeMap::new(),
            network_ingress_proxy_set_headers: BTreeMap::new(),
            network_ingress_proxy_connect_timeout_seconds: 60,
            network_ingress_proxy_send_timeout_seconds: 60,
            network_ingress_proxy_read_timeout_seconds: 60,
            network_ingress_proxy_request_buffering: "on".to_string(),
            network_ingress_proxy_buffering: "on".to_string(),
            network_ingress_proxy_buffer_size_kb: 4,
            network_ingress_whitelist_source_range: "0.0.0.0/0".to_string(),
            network_ingress_denylist_source_range: "".to_string(),
            network_ingress_basic_auth_env_var: "".to_string(),
            network_ingress_nginx_limit_rpm: None,
            network_ingress_nginx_limit_rps: None,
            network_ingress_nginx_limit_burst_multiplier: None,
            network_ingress_nginx_limit_connections: None,
            network_ingress_nginx_controller_server_snippet: None,
            network_ingress_nginx_controller_configuration_snippet: None,
            network_ingress_nginx_custom_http_errors: None,
            network_ingress_grpc_send_timeout_seconds: 60,
            network_ingress_grpc_read_timeout_seconds: 60,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HelmChartSource {
    Repository {
        engine_helm_registry: Box<Registry>,
        skip_tls_verify: bool,
        chart_name: String,
        chart_version: String,
    },
    Git {
        git_url: Url,
        git_credentials: Option<GitCredentials>,
        commit_id: String,
        root_path: PathBuf,
    },
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub struct HelmRawValues {
    pub name: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HelmValueSource {
    Raw {
        values: Vec<HelmRawValues>,
    },
    Git {
        git_url: Url,
        git_credentials: Option<GitCredentials>,
        commit_id: String,
        values_path: Vec<PathBuf>,
    },
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct HelmChart {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub action: Action,
    pub chart_source: HelmChartSource,
    pub chart_values: HelmValueSource,
    pub set_values: Vec<(String, String)>,
    pub set_string_values: Vec<(String, String)>,
    pub set_json_values: Vec<(String, String)>,
    pub command_args: Vec<String>,
    pub timeout_sec: u64,
    pub allow_cluster_wide_resources: bool,
    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    #[serde(default = "default_environment_vars_with_info")]
    pub environment_vars_with_infos: BTreeMap<String, VariableInfo>,
    pub advanced_settings: HelmChartAdvancedSettings,
    pub ports: Vec<Port>,
}

impl HelmChart {
    fn to_chart_source_domain(
        src: HelmChartSource,
        ssh_keys: &[SshKey],
        qovery_api: Arc<dyn QoveryApi>,
        service_id: Uuid,
    ) -> models::helm_chart::HelmChartSource {
        match src {
            HelmChartSource::Repository {
                engine_helm_registry,
                skip_tls_verify,
                chart_name,
                chart_version,
            } => models::helm_chart::HelmChartSource::Repository {
                engine_helm_registry,
                skip_tls_verify,
                chart_name,
                chart_version,
            },
            HelmChartSource::Git {
                git_url,
                git_credentials,
                commit_id,
                root_path,
            } => models::helm_chart::HelmChartSource::Git {
                git_url,
                get_credentials: if git_credentials.is_none() {
                    Box::new(|| Ok(None))
                } else {
                    Box::new(move || fetch_git_token(&*qovery_api, ServiceType::HelmChart, &service_id).map(Some))
                },
                commit_id,
                root_path,
                ssh_keys: ssh_keys.to_owned(),
            },
        }
    }

    fn to_chart_value_domain(
        src: HelmValueSource,
        ssh_keys: &[SshKey],
        qovery_api: Arc<dyn QoveryApi>,
        service_id: Uuid,
    ) -> models::helm_chart::HelmValueSource {
        match src {
            HelmValueSource::Raw { values } => models::helm_chart::HelmValueSource::Raw { values },
            HelmValueSource::Git {
                git_url,
                git_credentials,
                commit_id,
                values_path,
            } => models::helm_chart::HelmValueSource::Git {
                git_url,
                get_credentials: if git_credentials.is_none() {
                    Box::new(|| Ok(None))
                } else {
                    Box::new(move || fetch_git_token(&*qovery_api, ServiceType::HelmChart, &service_id).map(Some))
                },
                commit_id,
                values_path,
                ssh_keys: ssh_keys.to_owned(),
            },
        }
    }

    pub fn to_helm_chart_domain(
        self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
    ) -> Result<Box<dyn HelmChartService>, HelmChartError> {
        // Get passphrase and public key if provided by the user
        let ssh_keys: Vec<SshKey> = ssh_keys_from_env_vars(&self.environment_vars_with_infos.clone());
        let environment_variables_with_info: HashMap<String, VariableInfo> = self
            .environment_vars_with_infos
            .clone()
            .into_iter()
            .map(|(k, mut v)| {
                v.value = String::from_utf8_lossy(
                    &base64::engine::general_purpose::STANDARD
                        .decode(v.value)
                        .unwrap_or_default(),
                )
                .to_string();
                (k, v)
            })
            .collect();
        let service: Box<dyn HelmChartService> = match cloud_provider.kubernetes_kind() {
            kubernetes::Kind::Eks | kubernetes::Kind::EksSelfManaged => {
                Box::new(models::helm_chart::HelmChart::<AWS>::new(
                    context,
                    self.long_id,
                    self.name,
                    self.kube_name,
                    self.action.to_service_action(),
                    Self::to_chart_source_domain(
                        self.chart_source.clone(),
                        &ssh_keys,
                        context.qovery_api.clone(),
                        self.long_id,
                    ),
                    Self::to_chart_value_domain(self.chart_values, &ssh_keys, context.qovery_api.clone(), self.long_id),
                    self.set_values,
                    self.set_string_values,
                    self.set_json_values,
                    self.command_args,
                    std::time::Duration::from_secs(self.timeout_sec),
                    self.allow_cluster_wide_resources,
                    environment_variables_with_info,
                    self.advanced_settings,
                    AwsAppExtraSettings {},
                    |transmitter| context.get_event_details(transmitter),
                    self.ports,
                )?)
            }
            kubernetes::Kind::ScwKapsule | kubernetes::Kind::ScwSelfManaged => {
                Box::new(models::helm_chart::HelmChart::<SCW>::new(
                    context,
                    self.long_id,
                    self.name,
                    self.kube_name,
                    self.action.to_service_action(),
                    Self::to_chart_source_domain(
                        self.chart_source.clone(),
                        &ssh_keys,
                        context.qovery_api.clone(),
                        self.long_id,
                    ),
                    Self::to_chart_value_domain(self.chart_values, &ssh_keys, context.qovery_api.clone(), self.long_id),
                    self.set_values,
                    self.set_string_values,
                    self.set_json_values,
                    self.command_args,
                    std::time::Duration::from_secs(self.timeout_sec),
                    self.allow_cluster_wide_resources,
                    environment_variables_with_info,
                    self.advanced_settings,
                    ScwAppExtraSettings {},
                    |transmitter| context.get_event_details(transmitter),
                    self.ports,
                )?)
            }
            kubernetes::Kind::Gke | kubernetes::Kind::GkeSelfManaged => {
                Box::new(models::helm_chart::HelmChart::<GCP>::new(
                    context,
                    self.long_id,
                    self.name,
                    self.kube_name,
                    self.action.to_service_action(),
                    Self::to_chart_source_domain(
                        self.chart_source.clone(),
                        &ssh_keys,
                        context.qovery_api.clone(),
                        self.long_id,
                    ),
                    Self::to_chart_value_domain(self.chart_values, &ssh_keys, context.qovery_api.clone(), self.long_id),
                    self.set_values,
                    self.set_string_values,
                    self.set_json_values,
                    self.command_args,
                    std::time::Duration::from_secs(self.timeout_sec),
                    self.allow_cluster_wide_resources,
                    environment_variables_with_info,
                    self.advanced_settings,
                    GcpAppExtraSettings {},
                    |transmitter| context.get_event_details(transmitter),
                    self.ports,
                )?)
            }
            kubernetes::Kind::Aks | kubernetes::Kind::AksSelfManaged => {
                Box::new(models::helm_chart::HelmChart::<Azure>::new(
                    context,
                    self.long_id,
                    self.name,
                    self.kube_name,
                    self.action.to_service_action(),
                    Self::to_chart_source_domain(
                        self.chart_source.clone(),
                        &ssh_keys,
                        context.qovery_api.clone(),
                        self.long_id,
                    ),
                    Self::to_chart_value_domain(self.chart_values, &ssh_keys, context.qovery_api.clone(), self.long_id),
                    self.set_values,
                    self.set_string_values,
                    self.set_json_values,
                    self.command_args,
                    std::time::Duration::from_secs(self.timeout_sec),
                    self.allow_cluster_wide_resources,
                    environment_variables_with_info,
                    self.advanced_settings,
                    AzureAppExtraSettings {},
                    |transmitter| context.get_event_details(transmitter),
                    self.ports,
                )?)
            }
            kubernetes::Kind::OnPremiseSelfManaged => Box::new(models::helm_chart::HelmChart::<OnPremise>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                Self::to_chart_source_domain(
                    self.chart_source.clone(),
                    &ssh_keys,
                    context.qovery_api.clone(),
                    self.long_id,
                ),
                Self::to_chart_value_domain(self.chart_values, &ssh_keys, context.qovery_api.clone(), self.long_id),
                self.set_values,
                self.set_string_values,
                self.set_json_values,
                self.command_args,
                std::time::Duration::from_secs(self.timeout_sec),
                self.allow_cluster_wide_resources,
                environment_variables_with_info,
                self.advanced_settings,
                OnPremiseAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
                self.ports,
            )?),
        };

        Ok(service)
    }
}

#[cfg(test)]
mod tests {
    use crate::io_models::helm_chart::HelmChart;

    #[test]
    fn test_helm_deserialization_without_env_variables_with_infos() {
        let data = r#"
        {
            "long_id": "f84d837d-717e-4c39-bba4-573b22c5f848",
  "name": "name",
  "kube_name": "kube name",
  "action": "CREATE",
  "chart_source": {
    "git": {
      "git_url": "https://default.com/",
      "git_credentials": null,
      "commit_id": "",
      "root_path": ""
    }
  },
  "chart_values": {
    "raw": {
      "values": []
    }
  },
  "set_values": [],
  "set_string_values": [],
  "set_json_values": [],
  "command_args": [],
  "timeout_sec": 0,
  "allow_cluster_wide_resources": false,
  "environment_vars": { "key": "value" },
  "advanced_settings": {},
  "ports": [{"long_id":"5cfe6ff7-4907-40ff-9363-1c488fe5c8b1","port":9898,"name":"p9898","publicly_accessible":true,"is_default":false,"protocol":"HTTP","namespace":null,"service_name":null},
  {"long_id":"5cfe6ff7-4907-40ff-9363-1c488fe5c8b2","port":8080,"name":"p8080","publicly_accessible":true,"is_default":false,"protocol":"HTTP","namespace":"namespace_1","service_name":"service_1"}]
        }"#.to_string();

        let helm_chart: HelmChart = serde_json::from_str(data.as_str()).unwrap();
        assert_eq!(helm_chart.name, "name");
        assert_eq!(helm_chart.environment_vars_with_infos.len(), 0);
        assert_eq!(helm_chart.ports.len(), 2);
        assert!(
            helm_chart
                .ports
                .iter()
                .map(|port| (port.namespace.clone(), port.service_name.clone()))
                .any(|(namespace, service_name)| namespace.is_none() && service_name.is_none())
        );
        assert!(
            helm_chart
                .ports
                .iter()
                .map(|port| (port.namespace.clone(), port.service_name.clone()))
                .any(|(namespace, service_name)| namespace == Some("namespace_1".to_string())
                    && service_name == Some("service_1".to_string()))
        );
    }
}

#[test]
fn test_helm_deserialization_with_env_variables_with_infos() {
    let data = r#"
        {
            "long_id": "f84d837d-717e-4c39-bba4-573b22c5f848",
  "name": "name",
  "kube_name": "kube name",
  "action": "CREATE",
  "chart_source": {
    "git": {
      "git_url": "https://default.com/",
      "git_credentials": null,
      "commit_id": "",
      "root_path": ""
    }
  },
  "chart_values": {
    "raw": {
      "values": []
    }
  },
  "set_values": [],
  "set_string_values": [],
  "set_json_values": [],
  "command_args": [],
  "timeout_sec": 0,
  "allow_cluster_wide_resources": false,
  "environment_vars": { "key": "value" },
  "environment_vars_with_infos":{"variable":{"value":"value","is_secret":false},"secret":{"value":"my password","is_secret":true}},
  "advanced_settings": {},
  "ports": []
        }"#.to_string();

    let helm_chart: HelmChart = serde_json::from_str(data.as_str()).unwrap();
    assert_eq!(helm_chart.name, "name");
    assert_eq!(helm_chart.environment_vars_with_infos.len(), 2);
    assert_eq!(
        helm_chart.environment_vars_with_infos.get("variable").unwrap(),
        &VariableInfo {
            value: "value".to_string(),
            is_secret: false,
        }
    );
    assert_eq!(
        helm_chart.environment_vars_with_infos.get("secret").unwrap(),
        &VariableInfo {
            value: "my password".to_string(),
            is_secret: true,
        }
    );
}

#[test]
fn test_helm_deserialization_repository_source() {
    let data = r#"
        {
            "long_id": "f84d837d-717e-4c39-bba4-573b22c5f848",
  "name": "name",
  "kube_name": "kube name",
  "action": "CREATE",
  "chart_source": {
    "repository": {
        "url": "oci://default.com/",
        "credentials": {
            "login": "mon_nom",
            "password": "toto"
        },
        "engine_helm_registry": {
            "GenericCr": {
                "long_id": "bda696e6-de3a-4607-bd47-e8854e1c2880",
                "url": "oci://default.com/",
                "credentials": {
                    "login": "mon_nom",
                    "password": "toto"
                }
            }
        },
        "chart_name": "name of chart",
        "chart_version": "version of chart",
         "skip_tls_verify": false
        }
    },
  "chart_values": {
    "raw": {
      "values": []
    }
  },
  "set_values": [],
  "set_string_values": [],
  "set_json_values": [],
  "command_args": [],
  "timeout_sec": 0,
  "allow_cluster_wide_resources": false,
  "environment_vars": { "key": "value" },
  "advanced_settings": {},
  "ports": [{"long_id":"5cfe6ff7-4907-40ff-9363-1c488fe5c8b1","port":9898,"name":"p9898","publicly_accessible":true,"is_default":false,"protocol":"HTTP","namespace":null,"service_name":null},
  {"long_id":"5cfe6ff7-4907-40ff-9363-1c488fe5c8b2","port":8080,"name":"p8080","publicly_accessible":true,"is_default":false,"protocol":"HTTP","namespace":"namespace_1","service_name":"service_1"}]
        }"#.to_string();

    let helm_chart: HelmChart = serde_json::from_str(data.as_str()).unwrap();
    assert_eq!(helm_chart.name, "name");
    assert_eq!(helm_chart.environment_vars_with_infos.len(), 0);
    assert_eq!(helm_chart.ports.len(), 2);
    assert_eq!(
        helm_chart.chart_source,
        HelmChartSource::Repository {
            chart_name: "name of chart".to_string(),
            chart_version: "version of chart".to_string(),
            skip_tls_verify: false,
            engine_helm_registry: Box::new(Registry::GenericCr {
                long_id: Uuid::parse_str("bda696e6-de3a-4607-bd47-e8854e1c2880").unwrap(),
                url: Url::parse("oci://default.com/").unwrap(),
                credentials: Some(crate::io_models::container::Credentials {
                    login: "mon_nom".to_string(),
                    password: "toto".to_string()
                })
            })
        }
    );
}
