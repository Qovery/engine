use crate::cloud_provider::service::{Action, Database, IRouter, StatefulService, StatelessService};
use crate::models::application::IApplication;

pub struct Environment {
    namespace: String,
    pub id: String,
    pub project_id: String,
    pub owner_id: String,
    pub organization_id: String,
    pub action: Action,
    pub applications: Vec<Box<dyn IApplication>>,
    pub routers: Vec<Box<dyn IRouter>>,
    pub databases: Vec<Box<dyn Database>>,
}

impl Environment {
    pub fn new(
        id: &str,
        project_id: &str,
        owner_id: &str,
        organization_id: &str,
        action: Action,
        applications: Vec<Box<dyn IApplication>>,
        routers: Vec<Box<dyn IRouter>>,
        databases: Vec<Box<dyn Database>>,
    ) -> Self {
        Environment {
            namespace: format!("{}-{}", project_id, id),
            id: id.to_string(),
            project_id: project_id.to_string(),
            owner_id: owner_id.to_string(),
            organization_id: organization_id.to_string(),
            action,
            applications,
            routers,
            databases,
        }
    }

    pub fn stateless_services(&self) -> Vec<&dyn StatelessService> {
        let mut stateless_services: Vec<&dyn StatelessService> =
            Vec::with_capacity(self.applications.len() + self.routers.len());
        stateless_services.extend_from_slice(
            self.applications
                .iter()
                .map(|x| x.as_stateless_service())
                .collect::<Vec<_>>()
                .as_slice(),
        );
        stateless_services.extend_from_slice(
            self.routers
                .iter()
                .map(|x| x.as_stateless_service())
                .collect::<Vec<_>>()
                .as_slice(),
        );

        stateless_services
    }

    pub fn stateful_services(&self) -> Vec<&dyn StatefulService> {
        self.databases
            .iter()
            .map(|x| x.as_stateful_service())
            .collect::<Vec<_>>()
    }

    pub fn namespace(&self) -> &str {
        self.namespace.as_str()
    }
}
