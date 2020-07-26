use crate::cloud_provider::service::{
    Backup, Create, Delete, Downgrade, Service, ServiceError, StatefulService, StatelessService,
    Upgrade,
};
use std::borrow::Borrow;

pub struct Environment {
    namespace: String,
    pub kind: Kind,
    pub id: String,
    pub project_id: String,
    pub owner_id: String,
    pub organization_id: String,
    pub stateless_services: Vec<Box<dyn StatelessService>>,
    pub stateful_services: Vec<Box<dyn StatefulService>>,
}

impl Environment {
    pub fn new(
        kind: Kind,
        id: &str,
        project_id: &str,
        owner_id: &str,
        organization_id: &str,
        stateless_services: Vec<Box<dyn StatelessService>>,
        stateful_services: Vec<Box<dyn StatefulService>>,
    ) -> Self {
        Environment {
            namespace: format!("{}-{}", project_id, id),
            kind,
            id: id.to_string(),
            project_id: project_id.to_string(),
            owner_id: owner_id.to_string(),
            organization_id: organization_id.to_string(),
            stateless_services,
            stateful_services,
        }
    }

    pub fn namespace(&self) -> &str {
        self.namespace.as_str()
    }

    pub fn is_valid(&self) -> Result<(), ServiceError> {
        for service in self.stateful_services.iter() {
            match service.is_valid() {
                Err(err) => return Err(err),
                _ => {}
            }
        }

        for service in self.stateless_services.iter() {
            match service.is_valid() {
                Err(err) => return Err(err),
                _ => {}
            }
        }

        Ok(())
    }

    fn get_services_to_deploy<'a, T: Service + ?Sized>(
        &'a self,
        services: &'a Vec<Box<T>>,
    ) -> Vec<&'a Box<T>> {
        services
            .iter()
            .filter(|s| *s.action() == crate::cloud_provider::service::Action::Create)
            .collect::<Vec<_>>()
    }

    pub fn stateless_services_to_deploy<'a>(&'a self) -> Vec<&'a Box<dyn StatelessService>> {
        self.get_services_to_deploy(&self.stateless_services)
    }

    pub fn stateful_services_to_deploy<'a>(&'a self) -> Vec<&'a Box<dyn StatefulService>> {
        self.get_services_to_deploy(&self.stateful_services)
    }
}

pub enum Kind {
    Production,
    Development,
}
