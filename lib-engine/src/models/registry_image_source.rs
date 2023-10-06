use crate::io_models::container::Registry;
use crate::string::cut;
use uuid::Uuid;

pub struct RegistryImageSource {
    pub registry: Registry,
    pub image: String,
    pub tag: String,
    pub tag_for_mirror_with_service_id: bool,
}

impl RegistryImageSource {
    pub fn tag_for_mirror(&self, service_id: &Uuid) -> String {
        // A tag name must be valid ASCII and may contain lowercase and uppercase letters, digits, underscores, periods and dashes.
        // A tag name may not start with a period or a dash and may contain a maximum of 128 characters.
        match self.tag_for_mirror_with_service_id {
            true => cut(format!("{}.{}.{}", self.image.replace('/', "."), self.tag, service_id), 128),
            false => cut(format!("{}.{}", self.image.replace('/', "."), self.tag), 128),
        }
    }
}
