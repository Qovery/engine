use crate::cloud_provider::io::ImageMirroringMode;
use crate::io_models::container::Registry;
use crate::string::cut;
use uuid::Uuid;

pub struct RegistryImageSource {
    pub registry: Registry,
    pub image: String,
    pub tag: String,
    pub image_mirroring_mode: ImageMirroringMode,
}

impl RegistryImageSource {
    pub fn tag_for_mirror(&self, service_id: &Uuid) -> String {
        // A tag name must be valid ASCII and may contain lowercase and uppercase letters, digits, underscores, periods and dashes.
        // A tag name may not start with a period or a dash and may contain a maximum of 128 characters.
        match self.image_mirroring_mode {
            ImageMirroringMode::Service => {
                cut(format!("{}.{}.{}", self.image.replace('/', "."), self.tag, service_id), 128)
            }
            ImageMirroringMode::Cluster => cut(format!("{}.{}", self.image.replace('/', "."), self.tag), 128),
        }
    }
}
