mod database;
mod job;
mod router;
mod terraform_service;

use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::types::{Azure, CloudProvider};
use crate::infrastructure::models::cloud_provider::Kind;
use std::fmt::{Display, Formatter};

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
    pub resource_group_name: String,
}

#[derive(Clone, Eq, PartialEq)]
pub enum AzureStorageType {
    StandardLRS,
    StandardSSDZRS,
    PremiumLRS,
    PremiumV2LRS,
    PremiumZRS,
    StandardSSDLRS,
    UltraSSDLRS,
}

impl AzureStorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            AzureStorageType::StandardLRS => "azure-standard-lrs",
            AzureStorageType::StandardSSDZRS => "azure-standard-ssd-zrs",
            AzureStorageType::PremiumLRS => "azure-premium-lrs",
            AzureStorageType::PremiumV2LRS => "azure-premium-v2-lrs",
            AzureStorageType::PremiumZRS => "azure-premium-zrs",
            AzureStorageType::StandardSSDLRS => "azure-standard-ssd-lrs",
            AzureStorageType::UltraSSDLRS => "azure-ultra-ssd-lrs",
        }
        .to_string()
    }
}

impl ToCloudProviderFormat for AzureStorageType {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            AzureStorageType::StandardLRS => "Standard_LRS",
            AzureStorageType::StandardSSDZRS => "StandardSSD_ZRS",
            AzureStorageType::PremiumLRS => "Premium_LRS",
            AzureStorageType::PremiumV2LRS => "PremiumV2_LRS",
            AzureStorageType::PremiumZRS => "Premium_ZRS",
            AzureStorageType::StandardSSDLRS => "StandardSSD_LRS",
            AzureStorageType::UltraSSDLRS => "UltraSSD_LRS",
        }
    }
}

impl Display for AzureStorageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AzureStorageType::StandardLRS => write!(f, "Standard_LRS"),
            AzureStorageType::StandardSSDZRS => write!(f, "StandardSSD_ZRS"),
            AzureStorageType::PremiumLRS => write!(f, "Premium_LRS"),
            AzureStorageType::PremiumV2LRS => write!(f, "PremiumV2_LRS"),
            AzureStorageType::PremiumZRS => write!(f, "Premium_ZRS"),
            AzureStorageType::StandardSSDLRS => write!(f, "StandardSSD_LRS"),
            AzureStorageType::UltraSSDLRS => write!(f, "UltraSSD_LRS"),
        }
    }
}
