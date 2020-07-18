use crate::cloud_provider::service::{
    Backup, Create, Delete, Downgrade, Service, StatefulService, StatelessService, Upgrade,
};
use std::borrow::Borrow;

pub struct Environment {
    namespace: String,
    pub kind: Kind,
    pub id: String,
    pub project_id: String,
    pub owner_id: String,
    pub stateless_services: Vec<Box<dyn StatelessService>>,
    pub stateful_services: Vec<Box<dyn StatefulService>>,
}

impl Environment {
    pub fn new(
        kind: Kind,
        id: &str,
        project_id: &str,
        owner_id: &str,
        stateless_services: Vec<Box<dyn StatelessService>>,
        stateful_services: Vec<Box<dyn StatefulService>>,
    ) -> Self {
        Environment {
            namespace: format!("{}-{}", project_id, id),
            kind,
            id: id.to_string(),
            project_id: project_id.to_string(),
            owner_id: owner_id.to_string(),
            stateless_services,
            stateful_services,
        }
    }

    pub fn namespace(&self) -> &str {
        self.namespace.as_str()
    }
}

pub enum Kind {
    Production,
    Development,
}
