use aws_sdk_ec2::config::{BehaviorVersion, SharedCredentialsProvider};
use aws_types::SdkConfig;
use std::borrow::Cow;

use aws_types::region::Region;
use rusoto_core::{Client, HttpClient};
use rusoto_credential::StaticProvider;
use uuid::Uuid;

use crate::constants::{AWS_ACCESS_KEY_ID, AWS_DEFAULT_REGION, AWS_SECRET_ACCESS_KEY, AWS_SESSION_TOKEN};
use crate::infrastructure::models::cloud_provider::{
    CloudProvider, CloudProviderKind, Kind, TerraformStateCredentials,
};
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;

pub mod database_instance_type;
pub mod regions;

#[derive(Debug, Clone)]
pub enum AwsCredentials {
    Static {
        access_key_id: String,
        secret_access_key: String,
    },
    STS {
        access_key_id: String,
        secret_access_key: String,
        session_token: String,
    },
}

impl AwsCredentials {
    pub fn new(access_key_id: String, secret_access_key: String, session_token: Option<String>) -> Self {
        if let Some(session_token) = session_token {
            AwsCredentials::STS {
                access_key_id,
                secret_access_key,
                session_token,
            }
        } else {
            AwsCredentials::Static {
                access_key_id,
                secret_access_key,
            }
        }
    }

    pub fn access_key_id(&self) -> &str {
        match self {
            AwsCredentials::Static { access_key_id, .. } => access_key_id,
            AwsCredentials::STS { access_key_id, .. } => access_key_id,
        }
    }

    pub fn secret_access_key(&self) -> &str {
        match self {
            AwsCredentials::Static { secret_access_key, .. } => secret_access_key,
            AwsCredentials::STS { secret_access_key, .. } => secret_access_key,
        }
    }
    pub fn session_token(&self) -> Option<&str> {
        match self {
            AwsCredentials::Static { .. } => None,
            AwsCredentials::STS { session_token, .. } => Some(session_token),
        }
    }
}

pub struct AWS {
    long_id: Uuid,
    credentials: AwsCredentials,
    pub region: String,
    pub zones: Vec<String>,
    kubernetes_kind: KubernetesKind,
    terraform_state_credentials: TerraformStateCredentials,
}

impl AWS {
    pub fn new(
        long_id: Uuid,
        credentials: AwsCredentials,
        region: &str,
        zones: Vec<String>,
        kubernetes_kind: KubernetesKind,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Self {
        AWS {
            long_id,
            credentials,
            region: region.to_string(),
            zones,
            kubernetes_kind,
            terraform_state_credentials,
        }
    }

    pub fn aws_credentials(&self) -> &AwsCredentials {
        &self.credentials
    }

    pub fn client(&self) -> Client {
        Client::new_with(new_rusoto_creds(&self.credentials), HttpClient::new().unwrap())
    }

    pub fn aws_sdk_client(&self) -> SdkConfig {
        SdkConfig::builder()
            .credentials_provider(SharedCredentialsProvider::new(aws_credential_types::Credentials::new(
                self.credentials.access_key_id(),
                self.credentials.secret_access_key(),
                self.credentials.session_token().map(str::to_string),
                None,
                "qovery-engine",
            )))
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(Cow::from(self.region.clone())))
            .build()
    }
}

pub fn new_rusoto_creds(creds: &AwsCredentials) -> StaticProvider {
    StaticProvider::new(
        creds.access_key_id().to_string(),
        creds.secret_access_key().to_string(),
        creds.session_token().map(str::to_string),
        None,
    )
}

impl CloudProvider for AWS {
    fn kind(&self) -> Kind {
        Kind::Aws
    }

    fn kubernetes_kind(&self) -> KubernetesKind {
        self.kubernetes_kind
    }

    fn long_id(&self) -> Uuid {
        self.long_id
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        match &self.credentials {
            AwsCredentials::Static {
                access_key_id,
                secret_access_key,
            } => {
                vec![
                    (AWS_DEFAULT_REGION, self.region.as_str()),
                    (AWS_ACCESS_KEY_ID, access_key_id),
                    (AWS_SECRET_ACCESS_KEY, secret_access_key),
                ]
            }
            AwsCredentials::STS {
                access_key_id,
                secret_access_key,
                session_token,
            } => {
                vec![
                    (AWS_DEFAULT_REGION, self.region.as_str()),
                    (AWS_ACCESS_KEY_ID, access_key_id),
                    (AWS_SECRET_ACCESS_KEY, secret_access_key),
                    (AWS_SESSION_TOKEN, session_token),
                ]
            }
        }
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        match &self.credentials {
            AwsCredentials::Static {
                access_key_id,
                secret_access_key,
            } => {
                vec![("aws_access_key", access_key_id), ("aws_secret_key", secret_access_key)]
            }
            AwsCredentials::STS {
                access_key_id,
                secret_access_key,
                session_token,
            } => {
                vec![
                    ("aws_access_key", access_key_id),
                    ("aws_secret_key", secret_access_key),
                    ("aws_session_token", session_token),
                ]
            }
        }
    }

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        Some(&self.terraform_state_credentials)
    }

    fn downcast_ref(&self) -> CloudProviderKind {
        CloudProviderKind::Aws(self)
    }
}
