pub mod kapsule;
pub mod node;
pub mod public_gateway_type;

#[derive(Clone, Eq, PartialEq)]
pub enum ScwStorageType {
    SbvSsd,
}

impl ScwStorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            ScwStorageType::SbvSsd => "scw-sbv-ssd-0".to_string(),
        }
    }
}
