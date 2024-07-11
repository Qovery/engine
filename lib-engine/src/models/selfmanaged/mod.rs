use crate::cloud_provider::Kind;
use crate::models::types::CloudProvider;
use crate::models::types::OnPremise;
mod database;
mod job;
mod router;

pub struct OnPremiseAppExtraSettings {}
pub struct OnPremiseDbExtraSettings {}
pub struct OnPremiseRouterExtraSettings {}

impl CloudProvider for OnPremise {
    type AppExtraSettings = OnPremiseAppExtraSettings;
    type DbExtraSettings = OnPremiseDbExtraSettings;
    type RouterExtraSettings = OnPremiseRouterExtraSettings;
    fn cloud_provider() -> Kind {
        Kind::OnPremise
    }

    fn short_name() -> &'static str {
        "SelfManaged"
    }

    fn full_name() -> &'static str {
        "SelfManaged"
    }

    fn registry_short_name() -> &'static str {
        "SelfManaged"
    }

    fn registry_full_name() -> &'static str {
        "SelfManaged"
    }

    fn lib_directory_name() -> &'static str {
        "self-managed"
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OnPremiseStorageType {
    Local,
}

impl OnPremiseStorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            OnPremiseStorageType::Local => "local-path",
        }
        .to_string()
    }
}
