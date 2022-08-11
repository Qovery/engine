use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::CloudProvider;
use crate::container_registry::ContainerRegistry;
use crate::io_models::application::Application;
use crate::io_models::container::Container;
use crate::io_models::context::Context;
use crate::io_models::database::Database;
use crate::io_models::router::Router;
use crate::io_models::Action;
use crate::logger::Logger;
use crate::models::application::ApplicationError;
use crate::models::container::ContainerError;
use crate::models::database::DatabaseError;
use crate::models::router::RouterError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentRequest {
    pub execution_id: String,
    pub long_id: Uuid,
    pub project_long_id: Uuid,
    pub organization_long_id: Uuid,
    pub action: Action,
    pub applications: Vec<Application>,
    pub containers: Vec<Container>,
    pub routers: Vec<Router>,
    pub databases: Vec<Database>,
    pub clone_from_environment_id: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum DomainError {
    #[error("Invalid application: {0}")]
    ApplicationError(ApplicationError),
    #[error("Invalid container: {0}")]
    ContainerError(ContainerError),
    #[error("Invalid router: {0}")]
    RouterError(RouterError),
    #[error("Invalid database: {0}")]
    DatabaseError(DatabaseError),
}

impl EnvironmentRequest {
    pub fn to_environment_domain(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        container_registry: &dyn ContainerRegistry,
        logger: Box<dyn Logger>,
    ) -> Result<Environment, DomainError> {
        let mut applications = Vec::with_capacity(self.applications.len());
        for app in &self.applications {
            match app.to_application_domain(
                context,
                app.to_build(container_registry.registry_info()),
                cloud_provider,
                logger.clone(),
            ) {
                Ok(app) => applications.push(app),
                Err(err) => {
                    return Err(DomainError::ApplicationError(err));
                }
            }
        }

        let mut containers = Vec::with_capacity(self.containers.len());
        for container in &self.containers {
            match container
                .clone()
                .to_container_domain(context, cloud_provider, container_registry, logger.clone())
            {
                Ok(app) => containers.push(app),
                Err(err) => {
                    return Err(DomainError::ContainerError(err));
                }
            }
        }

        let mut routers = Vec::with_capacity(self.routers.len());
        for router in &self.routers {
            let mut custom_domain_check_enabled = true;
            for app in &self.applications {
                if !app.advanced_settings.deployment_custom_domain_check_enabled {
                    for route in &router.routes {
                        if route.service_long_id == app.long_id {
                            // disable custom domain check for this router
                            custom_domain_check_enabled = false;
                            break;
                        }
                    }
                }
            }

            match router.to_router_domain(context, custom_domain_check_enabled, cloud_provider, logger.clone()) {
                Ok(router) => routers.push(router),
                Err(err) => {
                    return Err(DomainError::RouterError(err));
                }
            }
        }

        let mut databases = Vec::with_capacity(self.databases.len());
        for db in &self.databases {
            match db.to_database_domain(context, cloud_provider, logger.clone()) {
                Ok(router) => databases.push(router),
                Err(err) => {
                    return Err(DomainError::DatabaseError(err));
                }
            }
        }

        Ok(Environment::new(
            self.long_id,
            self.project_long_id,
            self.organization_long_id,
            self.action.to_service_action(),
            applications,
            containers,
            routers,
            databases,
        ))
    }
}
