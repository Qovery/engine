extern crate digitalocean;
use crate::cloud_provider::{CloudProvider, CloudProviderError, Kind};
use crate::models::{Context, ProgressListener};
use digitalocean::DigitalOcean;
use std::any::Any;
use std::rc::Rc;

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

    fn is_valid(&self) -> Result<(), CloudProviderError> {
        unimplemented!()
    }

    fn add_listener(&mut self, _listener: Rc<Box<dyn ProgressListener>>) {
        unimplemented!()
    }

    fn as_any(&self) -> &dyn Any {
        unimplemented!()
    }
}
