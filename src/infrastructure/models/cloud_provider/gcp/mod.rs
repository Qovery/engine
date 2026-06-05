pub mod locations;

use crate::constants::{GCP_CREDENTIALS, GCP_OAUTH_ACCESS_TOKEN, GCP_PROJECT, GCP_REGION};
use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::gcp::{GcpCredentials, JsonCredentials};
use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use crate::infrastructure::models::cloud_provider::{
    CloudProvider, CloudProviderKind, Kind, TerraformStateCredentials,
};
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
use uuid::Uuid;

pub struct Google {
    long_id: Uuid,
    pub credentials: GcpCredentials,
    region: GcpRegion,
    terraform_state_credentials: TerraformStateCredentials,
}

impl Google {
    pub fn new(
        long_id: Uuid,
        json_credentials: JsonCredentials,
        region: GcpRegion,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Google {
        Self::new_with_credentials(long_id, json_credentials.into(), region, terraform_state_credentials)
    }

    pub fn new_with_credentials(
        long_id: Uuid,
        credentials: GcpCredentials,
        region: GcpRegion,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Google {
        Google {
            long_id,
            credentials,
            region,
            terraform_state_credentials,
        }
    }
}

impl CloudProvider for Google {
    fn kind(&self) -> Kind {
        Kind::Gcp
    }

    fn kubernetes_kind(&self) -> KubernetesKind {
        KubernetesKind::Gke
    }

    fn long_id(&self) -> Uuid {
        self.long_id
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        match &self.credentials {
            GcpCredentials::ServiceAccount(credentials) => vec![
                (GCP_CREDENTIALS, credentials.raw_json()),
                (GCP_PROJECT, credentials.project_id.as_str()),
                (GCP_REGION, self.region.to_cloud_provider_format()),
                credentials.cloudsdk_config(),
            ],
            GcpCredentials::AccessToken(credentials) => vec![
                (GCP_OAUTH_ACCESS_TOKEN, credentials.access_token.as_str()),
                (GCP_PROJECT, credentials.project_id.as_str()),
                (GCP_REGION, self.region.to_cloud_provider_format()),
                credentials.cloudsdk_config(),
            ],
        }
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            ("gcp_json_credentials", self.credentials.raw_json()),
            ("gcp_project_id", self.credentials.project_id()),
            ("gcp_region", self.region.to_cloud_provider_format()),
        ]
    }

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        Some(&self.terraform_state_credentials)
    }

    fn downcast_ref(&self) -> CloudProviderKind<'_> {
        CloudProviderKind::Gcp(self)
    }
}
