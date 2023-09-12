use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::io_models::application::to_environment_variable;
use crate::io_models::context::Context;
use crate::io_models::Action;
use crate::models;
use crate::models::aws::AwsAppExtraSettings;
use crate::models::aws_ec2::AwsEc2AppExtraSettings;
use crate::models::helm_chart::{HelmChartError, HelmChartService};
use crate::models::scaleway::ScwAppExtraSettings;
use crate::models::types::{AWSEc2, AWS, SCW};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Credentials {
    pub login: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum HelmRepository {
    Generic {
        long_id: Uuid,
        url: Url,
        credentials: Option<Credentials>,
    },
}

impl HelmRepository {
    pub fn url(&self) -> &Url {
        match self {
            Self::Generic { url, .. } => url,
        }
    }

    pub fn id(&self) -> &Uuid {
        match self {
            Self::Generic { long_id, .. } => long_id,
        }
    }

    pub fn get_url_with_credentials(&self) -> Url {
        match self {
            Self::Generic { url, credentials, .. } => {
                let mut url = url.clone();
                if let Some(credentials) = credentials {
                    let _ = url.set_username(&credentials.login);
                    let _ = url.set_password(Some(&credentials.password));
                }
                url
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
#[derive(Default)]
pub struct HelmChartAdvancedSettings {}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct HelmChart {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub action: Action,
    pub repository: HelmRepository,
    pub chart_name: String,
    pub chart_version: String,
    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    pub environment_vars: BTreeMap<String, String>,
    pub advanced_settings: HelmChartAdvancedSettings,
}

impl HelmChart {
    pub fn to_helm_chart_domain(
        self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
    ) -> Result<Box<dyn HelmChartService>, HelmChartError> {
        let environment_variables = to_environment_variable(self.environment_vars);
        let service: Box<dyn HelmChartService> = match cloud_provider.kubernetes_kind() {
            kubernetes::Kind::Eks => Box::new(models::helm_chart::HelmChart::<AWS>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                self.repository,
                self.chart_name,
                self.chart_version,
                environment_variables,
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
                self.repository,
                self.chart_name,
                self.chart_version,
                environment_variables,
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
                self.repository,
                self.chart_name,
                self.chart_version,
                environment_variables,
                self.advanced_settings,
                ScwAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
            )?),
        };

        Ok(service)
    }
}
