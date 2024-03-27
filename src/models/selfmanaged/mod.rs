use crate::cloud_provider::Kind;
use crate::models::types::CloudProvider;
use crate::models::types::OnPremise;
mod application;
mod container;
mod job;
mod router;

pub struct OnPremiseAppExtraSettings {}
pub struct OnPremiseDbExtraSettings {}
pub struct OnPremiseRouterExtraSettings {}

impl CloudProvider for OnPremise {
    type AppExtraSettings = OnPremiseAppExtraSettings;
    type DbExtraSettings = OnPremiseDbExtraSettings;
    type RouterExtraSettings = OnPremiseRouterExtraSettings;
    type StorageTypes = OnPremiseStorageType;

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
        "selfmanaged"
    }

    fn loadbalancer_l4_annotations() -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde_derive::Serialize, serde_derive::Deserialize)]
pub enum OnPremiseStorageType {}
