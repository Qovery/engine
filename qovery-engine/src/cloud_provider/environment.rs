use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Backup, Create, Delete, Downgrade, Service, StatefulService, StatelessService, Upgrade,
};
use crate::cloud_provider::CloudProvider;
use std::borrow::Borrow;

pub struct Environment<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
    pub kind: Kind,
    pub id: String,
    pub project_id: String,
    pub stateless_services: Vec<Box<dyn StatelessService<C, K>>>,
    pub stateful_services: Vec<Box<dyn StatefulService<C, K>>>,
}

impl<C, K> Environment<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
    pub fn new(id: &str, project_id: &str) -> Self {
        // FIXME TODO
        Environment {
            kind: Kind::Development,
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

pub enum Kind {
    Production,
    Development,
}
