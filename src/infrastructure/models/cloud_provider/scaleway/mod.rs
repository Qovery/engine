use uuid::Uuid;

use crate::constants::{SCW_ACCESS_KEY, SCW_DEFAULT_PROJECT_ID, SCW_SECRET_KEY};
use crate::infrastructure::models::cloud_provider::{
    CloudProvider, CloudProviderKind, Kind, TerraformStateCredentials,
};
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;

pub mod database_instance_type;

#[derive(Clone)]
pub struct ScalewayCredentials {
    pub access_key: String,
    pub secret_key: String,
    pub project_id: String,
}

pub struct Scaleway {
    long_id: Uuid,
    credentials: ScalewayCredentials,
    terraform_state_credentials: TerraformStateCredentials,
}

impl Scaleway {
    pub fn new(
        long_id: Uuid,
        access_key: &str,
        secret_key: &str,
        project_id: &str,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Scaleway {
        Scaleway {
            long_id,
            credentials: ScalewayCredentials {
                access_key: access_key.to_string(),
                secret_key: secret_key.to_string(),
                project_id: project_id.to_string(),
            },
            terraform_state_credentials,
        }
    }

    pub fn credentials(&self) -> &ScalewayCredentials {
        &self.credentials
    }
}

impl CloudProvider for Scaleway {
    fn kind(&self) -> Kind {
        Kind::Scw
    }

    fn kubernetes_kind(&self) -> KubernetesKind {
        KubernetesKind::ScwKapsule
    }

    fn long_id(&self) -> Uuid {
        self.long_id
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (SCW_ACCESS_KEY, self.credentials.access_key.as_str()),
            (SCW_SECRET_KEY, self.credentials.secret_key.as_str()),
            (SCW_DEFAULT_PROJECT_ID, self.credentials.project_id.as_str()),
        ]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            ("scaleway_access_key", self.credentials.access_key.as_str()),
            ("scaleway_secret_key", self.credentials.secret_key.as_str()),
            ("scaleway_project_id", self.credentials.project_id.as_str()),
        ]
    }

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        Some(&self.terraform_state_credentials)
    }

    fn downcast_ref(&self) -> CloudProviderKind {
        CloudProviderKind::Scw(self)
    }
}
