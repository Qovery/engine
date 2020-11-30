use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::Service;
use crate::cloud_provider::service::{Create, DatabaseOptions};
use crate::cloud_provider::DeploymentTarget;
use crate::error::{cast_simple_error_to_engine_error, EngineError};
use crate::models::{Action, Context, Environment};
use tera::Context as TeraContext;
pub struct PostgreSQL {
    context: Context,
    id: String,
    action: Action,
    name: String,
    version: String,
    fqdn: String,
    fqdn_id: String,
    total_cpus: String,
    total_ram_in_mib: u32,
    database_instance_type: String,
    options: DatabaseOptions,
}

impl PostgreSQL {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        version: &str,
        fqdn: &str,
        fqdn_id: &str,
        total_cpus: String,
        total_ram_in_mib: u32,
        database_instance_type: &str,
        options: DatabaseOptions,
    ) -> Self {
        PostgreSQL {
            context,
            action,
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            fqdn: fqdn.to_string(),
            fqdn_id: fqdn_id.to_string(),
            total_cpus,
            total_ram_in_mib,
            database_instance_type: database_instance_type.to_string(),
            options,
        }
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("postgresql-{}", self.id), 50)
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("databases/{}", self.name),
        )
    }
    fn tera_context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> TeraContext {
        let mut context = TeraContext::new();
        //TODO generate the context
        context
    }
}

impl Create for PostgreSQL {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!(
            "DigitalOcean.PostgreSQL.on_create() called for {}",
            self.name
        );
        let workspace_dir = self.workspace_directory();

        match target {
            DeploymentTarget::ManagedServices(kubernetes, environment) => {
                // use terraform
                info!("deploy postgresql on Digital Ocean Managed Services for {}",
                    self.name
                );
                unimplemented!()
            }
            DeploymentTarget::SelfHosted(kubernetes, environment) => {
                // use helm
                info!("deploy PostgreSQL on Kubernetes for {}",
                    self.name
                );
                unimplemented!()
            }
        }
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}
