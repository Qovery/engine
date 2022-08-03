use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::kubectl::kubectl_exec_is_namespace_present;
use crate::deployment_action::deploy_namespace::NamespaceDeployment;
use crate::deployment_action::DeploymentAction;
use crate::engine::EngineConfig;
use crate::errors::EngineError;
use crate::events::EventDetails;
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

pub struct EnvironmentDeployment<'a> {
    pub deployed_services: HashSet<Uuid>,
    deployment_target: DeploymentTarget<'a>,
    event_details: EventDetails,
}

impl<'a> EnvironmentDeployment<'a> {
    pub fn new(
        engine_config: &'a EngineConfig,
        environment: &'a Environment,
        event_details: EventDetails,
    ) -> Result<EnvironmentDeployment<'a>, EngineError> {
        let deployment_target = DeploymentTarget::new(engine_config, environment, &event_details)?;
        Ok(EnvironmentDeployment {
            deployed_services: Default::default(),
            deployment_target,
            event_details,
        })
    }

    pub fn on_create(&mut self) -> Result<(), EngineError> {
        let target = &self.deployment_target;
        let environment = &target.environment;

        // deploy namespace first
        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: self.event_details.clone(),
        };
        ns.exec_action(target, environment.action)?;

        // create all stateful services (database)
        for service in &environment.databases {
            self.deployed_services.insert(*service.long_id());
            service.exec_action(target, *service.action())?;
            service.exec_check_action(*service.action())?;
        }

        for service in &environment.containers {
            self.deployed_services.insert(*service.long_id());
            service.exec_action(target, *service.action())?;
            service.exec_check_action(*service.action())?;
        }

        // create all applications
        for service in &environment.applications {
            self.deployed_services.insert(*service.long_id());
            service.exec_action(target, *service.action())?;
            service.exec_check_action(*service.action())?;
        }

        // create all routers
        for service in &environment.routers {
            self.deployed_services.insert(*service.long_id());
            service.exec_action(target, *service.action())?;
            service.exec_check_action(*service.action())?;
        }

        Ok(())
    }

    pub fn on_pause(&mut self) -> Result<(), EngineError> {
        let target = &mut self.deployment_target;
        let environment = &target.environment;

        for service in &environment.routers {
            self.deployed_services.insert(*service.long_id());
            service.on_pause(target)?;
            service.on_pause_check()?;
        }

        for service in &environment.applications {
            self.deployed_services.insert(*service.long_id());
            service.on_pause(target)?;
            service.on_pause_check()?;
        }

        for service in &environment.containers {
            self.deployed_services.insert(*service.long_id());
            service.on_pause(target)?;
            service.on_pause_check()?;
        }

        for service in &environment.databases {
            self.deployed_services.insert(*service.long_id());
            service.on_pause(target)?;
            service.on_pause_check()?;
        }

        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: self.event_details.clone(),
        };
        ns.on_pause(target)?;

        Ok(())
    }

    pub fn on_delete(&mut self) -> Result<(), EngineError> {
        let target = &self.deployment_target;
        let environment = &target.environment;

        let kubeconfig = target.kubernetes.get_kubeconfig_file_path()?;

        // check if environment is not already deleted
        // speed up delete env because of terraform requiring apply + destroy
        if !kubectl_exec_is_namespace_present(
            kubeconfig,
            environment.namespace(),
            target.kubernetes.cloud_provider().credentials_environment_variables(),
        ) {
            info!("no need to delete environment {}, already absent", environment.namespace());
            return Ok(());
        };

        // delete all stateless services (router, application...)
        for service in &environment.routers {
            self.deployed_services.insert(*service.long_id());
            service.on_delete(target)?;
            service.on_delete_check()?;
        }

        for service in &environment.applications {
            self.deployed_services.insert(*service.long_id());
            service.on_delete(target)?;
            service.on_delete_check()?;
        }

        for service in &environment.containers {
            self.deployed_services.insert(*service.long_id());
            service.on_delete(target)?;
            service.on_delete_check()?;
        }

        // delete all stateful services (database)
        for service in &environment.databases {
            self.deployed_services.insert(*service.long_id());
            service.on_delete(target)?;
            service.on_delete_check()?
        }

        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: self.event_details.clone(),
        };
        ns.on_delete(target)?;

        Ok(())
    }
}
