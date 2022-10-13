use crate::cloud_provider::aws::load_balancers::clean_up_deleted_k8s_nlb;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::kubectl::kubectl_exec_is_namespace_present;
use crate::deployment_action::deploy_namespace::NamespaceDeployment;
use crate::deployment_action::DeploymentAction;
use crate::engine::EngineConfig;
use crate::errors::EngineError;
use crate::events::EnvironmentStep;
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

pub struct EnvironmentDeployment<'a> {
    pub deployed_services: HashSet<Uuid>,
    deployment_target: DeploymentTarget<'a>,
}

impl<'a> EnvironmentDeployment<'a> {
    pub fn new(
        engine_config: &'a EngineConfig,
        environment: &'a Environment,
        should_abort: &'a dyn Fn() -> bool,
    ) -> Result<EnvironmentDeployment<'a>, EngineError> {
        let deployment_target = DeploymentTarget::new(engine_config, environment, should_abort)?;
        Ok(EnvironmentDeployment {
            deployed_services: Default::default(),
            deployment_target,
        })
    }

    pub fn on_create(&mut self) -> Result<(), EngineError> {
        let target = &self.deployment_target;
        let environment = &target.environment;
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Deploy);
        let should_abort = || -> Result<(), EngineError> {
            if (target.should_abort)() {
                Err(EngineError::new_task_cancellation_requested(event_details.clone()))
            } else {
                Ok(())
            }
        };

        // deploy namespace first
        should_abort()?;
        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: event_details.clone(),
        };
        ns.exec_action(target, environment.action)?;

        // create all stateful services (database)
        for service in &environment.databases {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.exec_action(target, *service.action())?;
            service.exec_check_action(*service.action())?;
        }

        for service in &environment.containers {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.exec_action(target, *service.action())?;
            service.exec_check_action(*service.action())?;
        }

        // create all applications
        for service in &environment.applications {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.exec_action(target, *service.action())?;
            service.exec_check_action(*service.action())?;
        }

        // create all routers
        for service in &environment.routers {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.exec_action(target, *service.action())?;
            service.exec_check_action(*service.action())?;
        }

        // clean up nlb
        clean_up_deleted_k8s_nlb(event_details, target)?;

        Ok(())
    }

    pub fn on_pause(&mut self) -> Result<(), EngineError> {
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Pause);
        let target = &mut self.deployment_target;
        let environment = &target.environment;
        let should_abort = || -> Result<(), EngineError> {
            if (target.should_abort)() {
                Err(EngineError::new_task_cancellation_requested(event_details.clone()))
            } else {
                Ok(())
            }
        };

        for service in &environment.routers {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.on_pause(target)?;
            service.on_pause_check()?;
        }

        for service in &environment.applications {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.on_pause(target)?;
            service.on_pause_check()?;
        }

        for service in &environment.containers {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.on_pause(target)?;
            service.on_pause_check()?;
        }

        for service in &environment.databases {
            should_abort()?;
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
            event_details,
        };
        ns.on_pause(target)?;

        Ok(())
    }

    pub fn on_delete(&mut self) -> Result<(), EngineError> {
        let target = &self.deployment_target;
        let environment = &target.environment;
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Delete);
        let should_abort = || -> Result<(), EngineError> {
            if (target.should_abort)() {
                Err(EngineError::new_task_cancellation_requested(event_details.clone()))
            } else {
                Ok(())
            }
        };

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
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.on_delete(target)?;
            service.on_delete_check()?;
        }

        for service in &environment.applications {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.on_delete(target)?;
            service.on_delete_check()?;
        }

        for service in &environment.containers {
            should_abort()?;
            self.deployed_services.insert(*service.long_id());
            service.on_delete(target)?;
            service.on_delete_check()?;
        }

        // delete all stateful services (database)
        for service in &environment.databases {
            should_abort()?;
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
            event_details,
        };
        ns.on_delete(target)?;

        Ok(())
    }
}
