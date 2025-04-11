use crate::environment::models::azure::Credentials;
use crate::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use crate::infrastructure::models::cloud_provider::{
    CloudProvider, CloudProviderKind, Kind, TerraformStateCredentials,
};
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
use uuid::Uuid;

pub mod locations;

pub struct Azure {
    long_id: Uuid,
    _location: AzureLocation,
    pub credentials: Credentials,
    terraform_state_credentials: TerraformStateCredentials,
}

impl Azure {
    pub fn new(
        long_id: Uuid,
        location: AzureLocation,
        credentials: Credentials,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Self {
        Azure {
            long_id,
            _location: location,
            credentials,
            terraform_state_credentials,
        }
    }
}

impl CloudProvider for Azure {
    fn kind(&self) -> Kind {
        Kind::Azure
    }

    fn kubernetes_kind(&self) -> KubernetesKind {
        KubernetesKind::Aks
    }

    fn long_id(&self) -> uuid::Uuid {
        self.long_id
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![]
    }

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        Some(&self.terraform_state_credentials)
    }

    fn downcast_ref(&self) -> CloudProviderKind {
        CloudProviderKind::Azure(self)
    }
}
