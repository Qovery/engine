use crate::container_registry::{
    ContainerRegistry, ContainerRegistryError, Kind, PushError, PushResult,
};
use crate::models::{Context, ProgressListener};
use std::rc::Rc;
use crate::build_platform::Image;

pub struct DOCR {
    context: Context,
    registry_name: String,
    created_at: String,
}

impl DOCR {
    pub fn new(context: Context, registry_name: &str, created_at: &str) -> Self {
        DOCR {
            context,
            registry_name: registry_name.to_string(),
            created_at: created_at.to_string(),
        }
    }
}

impl ContainerRegistry for DOCR {
    fn context(&Self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
       Kind::DOCR
    }

    fn id(&self) -> &str {
        unimplemented!()
    }

    fn name(&self) -> &str {
        unimplemented!()
    }

    fn is_valid(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>) {
        unimplemented!()
    }

    fn on_create(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_create_error(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_delete_error(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        unimplemented!()
    }

    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, PushError> {
        unimplemented!()
    }

    fn push_error(&self, image: &Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }
}
