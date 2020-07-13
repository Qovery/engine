use crate::cloud_provider::application::Application;
use crate::cloud_provider::service::{
    Backup, Create, Delete, Downgrade, Service, StatefulService, StatelessService, Upgrade,
};
use std::borrow::Borrow;

pub struct Environment {
    pub id: String,
    pub project_id: String,
    pub stateless_services: Vec<Box<dyn StatelessService>>,
    pub stateful_services: Vec<Box<dyn StatefulService>>,
}

impl Environment {
    pub fn new(id: &str, project_id: &str) -> Self {
        Environment {
            id: id.to_string(),
            project_id: project_id.to_string(),
            stateless_services: vec![],
            stateful_services: vec![],
        }
    }

    pub fn namespace(&self) -> &str {
        self.project_id.as_str()
    }
}
