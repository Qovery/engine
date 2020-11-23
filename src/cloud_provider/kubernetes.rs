use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::CloudProvider;
use crate::dns_provider::DnsProvider;
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::models::{Context, Listener, Listeners};

pub trait Kubernetes {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn version(&self) -> &str;
    fn region(&self) -> &str;
    fn cloud_provider(&self) -> &dyn CloudProvider;
    fn dns_provider(&self) -> &dyn DnsProvider;
    fn is_valid(&self) -> Result<(), EngineError>;
    fn add_listener(&mut self, listener: Listener);
    fn listeners(&self) -> &Listeners;
    fn resources(&self, environment: &Environment) -> Result<Resources, EngineError>;
    fn on_create(&self) -> Result<(), EngineError>;
    fn on_create_error(&self) -> Result<(), EngineError>;
    fn on_upgrade(&self) -> Result<(), EngineError>;
    fn on_upgrade_error(&self) -> Result<(), EngineError>;
    fn on_downgrade(&self) -> Result<(), EngineError>;
    fn on_downgrade_error(&self) -> Result<(), EngineError>;
    fn on_delete(&self) -> Result<(), EngineError>;
    fn on_delete_error(&self) -> Result<(), EngineError>;
    fn deploy_environment(&self, environment: &Environment) -> Result<(), EngineError>;
    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), EngineError>;
    fn pause_environment(&self, environment: &Environment) -> Result<(), EngineError>;
    fn pause_environment_error(&self, environment: &Environment) -> Result<(), EngineError>;
    fn delete_environment(&self, environment: &Environment) -> Result<(), EngineError>;
    fn delete_environment_error(&self, environment: &Environment) -> Result<(), EngineError>;
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Kubernetes(self.id().to_string(), self.name().to_string())
    }
    fn engine_error(&self, cause: EngineErrorCause, message: String) -> EngineError {
        EngineError::new(
            cause,
            self.engine_error_scope(),
            self.context().execution_id(),
            Some(message),
        )
    }
}

pub trait KubernetesNode {
    fn total_cpu(&self) -> u8;
    fn total_memory_in_gib(&self) -> u16;
    fn instance_type(&self) -> &str;
    fn as_any(&self) -> &dyn Any;
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Kind {
    EKS,
    DOKS,
}

#[derive(Debug)]
pub struct Resources {
    pub free_cpu: f32,
    pub max_cpu: f32,
    pub free_ram_in_mib: u32,
    pub max_ram_in_mib: u32,
    pub free_pods: u16,
    pub max_pods: u16,
    pub running_nodes: u16,
}

/// check that there is enough CPU and RAM, and pods resources
/// before starting to deploy stateful and stateless services
pub fn check_kubernetes_has_enough_resources_to_deploy_environment(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
) -> Result<(), EngineError> {
    let resources = kubernetes.resources(environment)?;
    let required_resources = environment.required_resources();

    let cause = EngineErrorCause::User("Contact your Organization administrator and consider to \
            add one more node or upgrade your nodes configuration. If not possible, pause or delete unused environments");

    if required_resources.cpu > resources.free_cpu
        && required_resources.ram_in_mib > resources.free_ram_in_mib
    {
        // not enough cpu and ram to deploy environment
        let message = format!(
            "There is not enough CPU and RAM resources on the Kubernetes '{}' cluster. \
                {} CPU and {}mib RAM requested. \
                {} CPU and {}mib RAM available.",
            kubernetes.name(),
            required_resources.cpu,
            required_resources.ram_in_mib,
            resources.free_cpu,
            resources.free_ram_in_mib,
        );

        return Err(kubernetes.engine_error(cause, message));
    } else if required_resources.cpu > resources.free_cpu {
        // not enough cpu to deploy environment
        let message = format!(
            "There is not enough free CPU on the Kubernetes '{}' cluster. \
                {} CPU requested. {} CPU available. \
                Consider to add one more node or upgrade your nodes configuration.",
            kubernetes.name(),
            required_resources.cpu,
            resources.free_cpu,
        );

        return Err(kubernetes.engine_error(cause, message));
    } else if required_resources.ram_in_mib > resources.free_ram_in_mib {
        // not enough ram to deploy environment
        let message = format!(
            "There is not enough free RAM on the Kubernetes cluster '{}'. \
                {}mib RAM requested. \
                {}mib RAM available. \
                Consider to add one more node or upgrade your nodes configuration.",
            kubernetes.name(),
            required_resources.ram_in_mib,
            resources.free_ram_in_mib,
        );

        return Err(kubernetes.engine_error(cause, message));
    }

    let mut required_pods = environment.stateless_services.len() as u16;

    match environment.kind {
        crate::cloud_provider::environment::Kind::Production => {}
        crate::cloud_provider::environment::Kind::Development => {
            required_pods += environment.stateful_services.len() as u16;
        }
    }

    if required_pods > resources.free_pods {
        // not enough free pods on the cluster
        let message = format!(
            "There is not enough free Pods ({} required) on the Kubernetes cluster '{}'. \
                Consider to add one more node or upgrade your nodes configuration.",
            required_pods,
            kubernetes.name(),
        );

        return Err(kubernetes.engine_error(cause, message));
    }

    Ok(())
}
