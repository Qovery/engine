use uuid::Uuid;

use crate::infrastructure::models::cloud_provider::{
    CloudProvider, CloudProviderKind, Kind, TerraformStateCredentials,
};
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;

pub struct SelfManaged {
    long_id: Uuid,
    vsphere_user: Option<String>,
    vsphere_password: Option<String>,
}

impl SelfManaged {
    pub fn new(long_id: Uuid, vsphere_user: Option<String>, vsphere_password: Option<String>) -> Self {
        SelfManaged {
            long_id,
            vsphere_user,
            vsphere_password,
        }
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
        let mut envs = Vec::with_capacity(6);

        if let Some(vsphere_user) = &self.vsphere_user {
            envs.push(("GOVC_USERNAME", vsphere_user.as_str()));
            envs.push(("VSPHERE_USERNAME", vsphere_user.as_str()));
            envs.push(("EKSA_VSPHERE_USERNAME", vsphere_user.as_str()));
        }

        if let Some(vsphere_password) = &self.vsphere_password {
            envs.push(("GOVC_PASSWORD", vsphere_password.as_str()));
            envs.push(("VSPHERE_PASSWORD", vsphere_password.as_str()));
            envs.push(("EKSA_VSPHERE_PASSWORD", vsphere_password.as_str()));
        }

        envs
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        None
    }

    fn downcast_ref(&self) -> CloudProviderKind<'_> {
        CloudProviderKind::SelfManaged(self)
    }
}
