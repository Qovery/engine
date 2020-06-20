use crate::registry::Registry;

pub struct DockerHub {}

impl Registry for DockerHub {
    fn is_valid(&self) -> bool {
        true
    }
}
