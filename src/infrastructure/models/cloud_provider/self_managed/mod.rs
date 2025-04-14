use uuid::Uuid;

use crate::infrastructure::models::cloud_provider::{
    CloudProvider, CloudProviderKind, Kind, TerraformStateCredentials,
};
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;

pub struct SelfManaged {
    long_id: Uuid,
}

impl SelfManaged {
    pub fn new(long_id: Uuid) -> Self {
        SelfManaged { long_id }
    }
}

impl CloudProvider for SelfManaged {
    fn kind(&self) -> Kind {
        Kind::OnPremise
    }

    fn kubernetes_kind(&self) -> KubernetesKind {
        KubernetesKind::OnPremiseSelfManaged
    }

    fn long_id(&self) -> Uuid {
        self.long_id
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        None
    }

    fn downcast_ref(&self) -> CloudProviderKind {
        CloudProviderKind::SelfManaged(self)
    }
}
