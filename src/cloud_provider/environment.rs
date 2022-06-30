use crate::cloud_provider::service::{Action, DatabaseService, RouterService, StatelessService};
use crate::models::application::ApplicationService;
use crate::utilities::to_short_id;
use uuid::Uuid;

pub struct Environment {
    namespace: String,
    pub id: String,
    pub long_id: Uuid,
    pub project_id: String,
    pub project_long_id: Uuid,
    pub owner_id: String,
    pub organization_id: String,
    pub organization_long_id: Uuid,
    pub action: Action,
    pub applications: Vec<Box<dyn ApplicationService>>,
    pub routers: Vec<Box<dyn RouterService>>,
    pub databases: Vec<Box<dyn DatabaseService>>,
}

impl Environment {
    pub fn new(
        long_id: Uuid,
        project_long_id: Uuid,
        organization_long_id: Uuid,
        action: Action,
        applications: Vec<Box<dyn ApplicationService>>,
        routers: Vec<Box<dyn RouterService>>,
        databases: Vec<Box<dyn DatabaseService>>,
    ) -> Self {
        let project_id = to_short_id(&project_long_id);
        let env_id = to_short_id(&long_id);
        Environment {
            namespace: format!("{}-{}", project_id, env_id),
            id: env_id,
            long_id,
            project_id,
            project_long_id,
            owner_id: "FAKE".to_string(),
            organization_id: to_short_id(&organization_long_id),
            organization_long_id,
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

    pub fn namespace(&self) -> &str {
        self.namespace.as_str()
    }
}
