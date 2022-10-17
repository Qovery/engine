use crate::cloud_provider::aws::load_balancers::clean_up_deleted_k8s_nlb;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::kubectl::kubectl_exec_is_namespace_present;
use crate::deployment_action::deploy_namespace::NamespaceDeployment;
use crate::deployment_action::DeploymentAction;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails};
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

pub struct EnvironmentDeployment<'a> {
    pub deployed_services: HashSet<Uuid>,
    deployment_target: DeploymentTarget<'a>,
}

impl<'a> EnvironmentDeployment<'a> {
    pub fn new(
        infra_ctx: &'a InfrastructureContext,
        environment: &'a Environment,
        should_abort: &'a dyn Fn() -> bool,
    ) -> Result<EnvironmentDeployment<'a>, EngineError> {
        let deployment_target = DeploymentTarget::new(infra_ctx, environment, should_abort)?;
        Ok(EnvironmentDeployment {
            deployed_services: HashSet::with_capacity(Self::services_iter(environment).count()),
            deployment_target,
        })
    }

    fn services_iter(
        environment: &Environment,
    ) -> impl DoubleEndedIterator<Item = (Uuid, &dyn DeploymentAction, Action)> {
        std::iter::empty()
            .chain(
                environment
                    .databases
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .containers
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .applications
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .routers
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
    }

    fn should_abort_wrapper<'b>(
        target: &'b DeploymentTarget,
        event_details: &'b EventDetails,
    ) -> impl Fn() -> Result<(), EngineError> + 'b {
        move || {
            if (target.should_abort)() {
                Err(EngineError::new_task_cancellation_requested(event_details.clone()))
            } else {
                Ok(())
            }
        }
    }

    pub fn on_create(&mut self) -> Result<(), EngineError> {
        let target = &self.deployment_target;
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Deploy);
        let resource_expiration = target
            .kubernetes
            .context()
            .resource_expiration_in_seconds()
            .map(|ttl| Duration::from_secs(ttl as u64));
        let should_abort = Self::should_abort_wrapper(target, &event_details);

        // deploy namespace first
        should_abort()?;
        let ns = NamespaceDeployment {
            resource_expiration,
            event_details: event_details.clone(),
        };
        ns.exec_action(target, target.environment.action)?;

        let services = Self::services_iter(target.environment);
        for (service_id, service, service_action) in services {
            should_abort()?;
            self.deployed_services.insert(service_id);
            service.exec_action(target, service_action)?;
            service.exec_check_action(service_action, target)?;
        }

        // clean up nlb
        clean_up_deleted_k8s_nlb(event_details.clone(), target)?;

        Ok(())
    }

    pub fn on_pause(&mut self) -> Result<(), EngineError> {
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Pause);
        let target = &mut self.deployment_target;
        let should_abort = Self::should_abort_wrapper(target, &event_details);

        // reverse order of the deployment
        let services = Self::services_iter(target.environment).rev();
        for (service_id, service, _) in services {
            should_abort()?;
            self.deployed_services.insert(service_id);
            service.on_pause(target)?;
            service.on_pause_check(target)?;
        }

        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: event_details.clone(),
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

        // check if environment is not already deleted
        // speed up delete env because of terraform requiring apply + destroy
        if !kubectl_exec_is_namespace_present(
            target.kubernetes.get_kubeconfig_file_path()?,
            environment.namespace(),
            target.kubernetes.cloud_provider().credentials_environment_variables(),
        ) {
            info!("no need to delete environment {}, already absent", environment.namespace());
            Self::services_iter(target.environment).for_each(|(id, _, _)| {
                self.deployed_services.insert(id);
            });
            return Ok(());
        };

        // reverse order of the deployment
        let should_abort = Self::should_abort_wrapper(target, &event_details);
        let services = Self::services_iter(target.environment).rev();
        for (service_id, service, _) in services {
            should_abort()?;
            self.deployed_services.insert(service_id);
            service.on_delete(target)?;
            service.on_delete_check(target)?;
        }

        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: event_details.clone(),
        };
        ns.on_delete(target)?;

        Ok(())
    }
}
