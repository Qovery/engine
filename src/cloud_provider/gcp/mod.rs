use std::any::Any;

use crate::cloud_provider::{CloudProvider, Kind, TerraformStateCredentials};
use crate::error::EngineError;
use crate::models::{Context, Listener, ProgressListener};

pub struct GCP {
    context: Context,
    id: String,
    name: String,
    p12_file_content: String,
}

impl GCP {
    pub fn new(context: Context, id: &str, name: &str, p12_file_content: &str) -> Self {
        GCP {
            context,
            id: id.to_string(),
            name: name.to_string(),
            p12_file_content: p12_file_content.to_string(),
        }
    }
}

impl<'x> CloudProvider for GCP {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::GCP
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn organization_id(&self) -> &str {
        unimplemented!()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn add_listener(&mut self, _listener: Listener) {
        // TODO
    }

    fn terraform_state_credentials(&self) -> &TerraformStateCredentials {
        unimplemented!()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
