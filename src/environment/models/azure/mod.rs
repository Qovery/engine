mod database;
mod job;
mod router;
mod terraform_service;

use crate::environment::models::types::{Azure, CloudProvider};
use crate::infrastructure::models::cloud_provider::Kind;

pub struct AzureAppExtraSettings {}
pub struct AzureDbExtraSettings {}
pub struct AzureRouterExtraSettings {}

impl CloudProvider for Azure {
    type AppExtraSettings = AzureAppExtraSettings;
    type DbExtraSettings = AzureDbExtraSettings;
    type RouterExtraSettings = AzureRouterExtraSettings;

    fn cloud_provider() -> Kind {
        Kind::Azure
    }

    fn short_name() -> &'static str {
        "Azure"
    }

    fn full_name() -> &'static str {
        "Microsoft Azure"
    }

    fn registry_short_name() -> &'static str {
        "ACR"
    }

    fn registry_full_name() -> &'static str {
        "Azure Container Registry"
    }

    fn lib_directory_name() -> &'static str {
        "azure"
    }
}

#[derive(Clone)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
    pub tenant_id: String,
    pub subscription_id: String,
}
