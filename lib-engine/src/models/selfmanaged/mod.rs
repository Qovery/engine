use crate::cloud_provider::Kind;
use crate::models::types::CloudProvider;
use crate::models::types::SelfManaged;
mod application;
mod container;
mod job;
mod router;

pub struct SelfManagedAppExtraSettings {}
pub struct SelfManagedDbExtraSettings {}
pub struct SelfManagedRouterExtraSettings {}

impl CloudProvider for SelfManaged {
    type AppExtraSettings = SelfManagedAppExtraSettings;
    type DbExtraSettings = SelfManagedDbExtraSettings;
    type RouterExtraSettings = SelfManagedRouterExtraSettings;
    type StorageTypes = SelfManagedStorageType;

    fn cloud_provider() -> Kind {
        Kind::SelfManaged
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
pub enum SelfManagedStorageType {}
