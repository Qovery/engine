extern crate digitalocean;

use std::any::Any;

use digitalocean::DigitalOcean;

use crate::cloud_provider::{CloudProvider, Kind, TerraformStateCredentials};
use crate::error::EngineError;
use crate::models::{Context, Listener};

pub struct DO {
    context: Context,
    id: String,
    pub token: String,
}

impl DO {
    pub fn new(context: Context, id: &str, token: &str) -> Self {
        DO {
            context,
            id: id.to_string(),
            token: token.to_string(),
        }
    }

    pub fn client(&self) -> DigitalOcean {
        DigitalOcean::new(self.token.as_str()).unwrap()
    }
}

impl CloudProvider for DO {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::DO
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn organization_id(&self) -> &str {
        unimplemented!()
    }

    fn name(&self) -> &str {
        unimplemented!()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn add_listener(&mut self, _listener: Listener) {
        unimplemented!()
    }

    fn terraform_state_credentials(&self) -> &TerraformStateCredentials {
        unimplemented!()
    }

    fn as_any(&self) -> &dyn Any {
        unimplemented!()
    }
}
