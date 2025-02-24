pub mod locations;

use crate::constants::{GCP_CREDENTIALS, GCP_PROJECT, GCP_REGION};
use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::gcp::JsonCredentials;
use crate::environment::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use crate::infrastructure::models::cloud_provider::{
    CloudProvider, CloudProviderKind, Kind, TerraformStateCredentials,
};
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
use uuid::Uuid;

pub struct Google {
    long_id: Uuid,
    pub json_credentials: JsonCredentials,
    json_credentials_raw_json: String,
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
        let credentials_io = JsonCredentialsIo::from(json_credentials.clone());

        Google {
            long_id,
            json_credentials,
            json_credentials_raw_json: serde_json::to_string(&credentials_io).unwrap_or_default(),
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
        vec![
            (GCP_CREDENTIALS, self.json_credentials_raw_json.as_str()),
            (GCP_PROJECT, self.json_credentials.project_id.as_str()),
            (GCP_REGION, self.region.to_cloud_provider_format()),
        ]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            ("gcp_json_credentials", self.json_credentials_raw_json.as_str()),
            ("gcp_project_id", self.json_credentials.project_id.as_str()),
            ("gcp_region", self.region.to_cloud_provider_format()),
        ]
    }

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        Some(&self.terraform_state_credentials)
    }

    fn downcast_ref(&self) -> CloudProviderKind {
        CloudProviderKind::Gcp(self)
    }
}
