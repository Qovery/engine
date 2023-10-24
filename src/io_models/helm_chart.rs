use crate::build_platform::SshKey;
use crate::cloud_provider::service::ServiceType;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::engine_task::qovery_api::QoveryApi;
use crate::io_models::application::GitCredentials;
use crate::io_models::context::Context;
use crate::io_models::variable_utils::{default_environment_vars_with_info, VariableInfo};
use crate::io_models::{fetch_git_token, ssh_keys_from_env_vars, Action};
use crate::models;
use crate::models::aws::AwsAppExtraSettings;
use crate::models::aws_ec2::AwsEc2AppExtraSettings;
use crate::models::helm_chart::{HelmChartError, HelmChartService};
use crate::models::scaleway::ScwAppExtraSettings;
use crate::models::types::{AWSEc2, AWS, SCW};
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
#[derive(Default)]
pub struct HelmChartAdvancedSettings {}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HelmChartSource {
    Repository {
        url: Url,
        credentials: Option<HelmCredentials>,
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
                url,
                credentials,
                skip_tls_verify,
                chart_name,
                chart_version,
            } => models::helm_chart::HelmChartSource::Repository {
                url,
                credentials,
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
        let environment_variables_with_infos: HashMap<String, VariableInfo> =
            self.environment_vars_with_infos.clone().into_iter().collect();
        let service: Box<dyn HelmChartService> = match cloud_provider.kubernetes_kind() {
            kubernetes::Kind::Eks => Box::new(models::helm_chart::HelmChart::<AWS>::new(
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
                environment_variables_with_infos,
                self.advanced_settings,
                AwsAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
            )?),
            kubernetes::Kind::Ec2 => Box::new(models::helm_chart::HelmChart::<AWSEc2>::new(
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
                environment_variables_with_infos,
                self.advanced_settings,
                AwsEc2AppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
            )?),
            kubernetes::Kind::ScwKapsule => Box::new(models::helm_chart::HelmChart::<SCW>::new(
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
                environment_variables_with_infos,
                self.advanced_settings,
                ScwAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
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
        let data = format!(
            r#"
        {{
            "long_id": "f84d837d-717e-4c39-bba4-573b22c5f848",
  "name": "name",
  "kube_name": "kube name",
  "action": "CREATE",
  "chart_source": {{
    "git": {{
      "git_url": "https://default.com/",
      "git_credentials": null,
      "commit_id": "",
      "root_path": ""
    }}
  }},
  "chart_values": {{
    "raw": {{
      "values": []
    }}
  }},
  "set_values": [],
  "set_string_values": [],
  "set_json_values": [],
  "command_args": [],
  "timeout_sec": 0,
  "allow_cluster_wide_resources": false,
  "environment_vars": {{ "key": "value" }},
  "advanced_settings": {{}}
        }}"#
        );

        let helm_chart: HelmChart = serde_json::from_str(data.as_str()).unwrap();
        assert_eq!(helm_chart.name, "name");
        assert_eq!(helm_chart.environment_vars_with_infos.len(), 0);
    }
}

#[test]
fn test_helm_deserialization_with_env_variables_with_infos() {
    let data = format!(
        r#"
        {{
            "long_id": "f84d837d-717e-4c39-bba4-573b22c5f848",
  "name": "name",
  "kube_name": "kube name",
  "action": "CREATE",
  "chart_source": {{
    "git": {{
      "git_url": "https://default.com/",
      "git_credentials": null,
      "commit_id": "",
      "root_path": ""
    }}
  }},
  "chart_values": {{
    "raw": {{
      "values": []
    }}
  }},
  "set_values": [],
  "set_string_values": [],
  "set_json_values": [],
  "command_args": [],
  "timeout_sec": 0,
  "allow_cluster_wide_resources": false,
  "environment_vars": {{ "key": "value" }},
  "environment_vars_with_infos":{{"variable":{{"value":"value","is_secret":false}},"secret":{{"value":"my password","is_secret":true}}}},
  "advanced_settings": {{}}
        }}"#
    );

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
