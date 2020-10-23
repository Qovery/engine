use std::any::Any;
use std::process::ExitStatus;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service::ServiceError;
use crate::cloud_provider::CloudProvider;
use crate::cmd::utilities::CmdError;
use crate::dns_provider::DnsProvider;
use crate::models::{Context, Listener, Listeners, ProgressListener};

pub trait Kubernetes {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn region(&self) -> &str;
    fn cloud_provider(&self) -> &dyn CloudProvider;
    fn dns_provider(&self) -> &dyn DnsProvider;
    fn is_valid(&self) -> Result<(), KubernetesError>;
    fn add_listener(&mut self, listener: Listener);
    fn listeners(&self) -> &Listeners;
    fn resources(&self, environment: &Environment) -> Result<Resources, KubernetesError>;
    fn on_create(&self) -> Result<(), KubernetesError>;
    fn on_create_error(&self) -> Result<(), KubernetesError>;
    fn on_upgrade(&self) -> Result<(), KubernetesError>;
    fn on_upgrade_error(&self) -> Result<(), KubernetesError>;
    fn on_downgrade(&self) -> Result<(), KubernetesError>;
    fn on_downgrade_error(&self) -> Result<(), KubernetesError>;
    fn on_delete(&self) -> Result<(), KubernetesError>;
    fn on_delete_error(&self) -> Result<(), KubernetesError>;
    fn deploy_environment(&self, environment: &Environment) -> Result<(), KubernetesError>;
    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), KubernetesError>;
    fn pause_environment(&self, environment: &Environment) -> Result<(), KubernetesError>;
    fn pause_environment_error(&self, environment: &Environment) -> Result<(), KubernetesError>;
    fn delete_environment(&self, environment: &Environment) -> Result<(), KubernetesError>;
    fn delete_environment_error(&self, environment: &Environment) -> Result<(), KubernetesError>;
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

#[derive(Debug)]
pub enum KubernetesError {
    Cmd(CmdError),
    Io(std::io::Error),
    Create(ExitStatus),
    Deploy(ServiceError),
    Pause(ServiceError),
    Delete(ServiceError),
    Error,
}

impl From<std::io::Error> for KubernetesError {
    fn from(error: std::io::Error) -> Self {
        KubernetesError::Io(error)
    }
}

impl From<CmdError> for KubernetesError {
    fn from(error: CmdError) -> Self {
        KubernetesError::Cmd(error)
    }
}

impl From<KubernetesError> for Option<ServiceError> {
    fn from(item: KubernetesError) -> Self {
        return match item {
            KubernetesError::Deploy(e) | KubernetesError::Pause(e) | KubernetesError::Delete(e) => {
                Option::from(e)
            }
            _ => None,
        };
    }
}

/// check that there is enough CPU and RAM, and pods resources
/// before starting to deploy stateful and stateless services
pub fn check_kubernetes_has_enough_resources_to_deploy_environment(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
) -> Result<(), KubernetesError> {
    let resources = kubernetes.resources(environment)?;
    let required_resources = environment.required_resources();

    if required_resources.cpu > resources.free_cpu
        && required_resources.ram_in_mib > resources.free_ram_in_mib
    {
        // not enough cpu and ram to deploy environment
        return Err(KubernetesError::Deploy(ServiceError::NotEnoughResources(
            format!(
                "There is not enough CPU and RAM resources on the Kubernetes '{}' cluster. \
                {} CPU and {}mib RAM requested. \
                {} CPU and {}mib RAM available. \
                Consider to add one more node or upgrade your nodes configuration.",
                kubernetes.name(),
                required_resources.cpu,
                required_resources.ram_in_mib,
                resources.free_cpu,
                resources.free_ram_in_mib,
            ),
        )));
    } else if required_resources.cpu > resources.free_cpu {
        // not enough cpu to deploy environment
        return Err(KubernetesError::Deploy(ServiceError::NotEnoughResources(
            format!(
                "There is not enough free CPU on the Kubernetes '{}' cluster. \
                {} CPU requested. {} CPU available. \
                Consider to add one more node or upgrade your nodes configuration.",
                kubernetes.name(),
                required_resources.cpu,
                resources.free_cpu,
            ),
        )));
    } else if required_resources.ram_in_mib > resources.free_ram_in_mib {
        // not enough ram to deploy environment
        return Err(KubernetesError::Deploy(ServiceError::NotEnoughResources(
            format!(
                "There is not enough free RAM on the Kubernetes cluster '{}'. \
                {}mib RAM requested. \
                {}mib RAM available. \
                Consider to add one more node or upgrade your nodes configuration.",
                kubernetes.name(),
                required_resources.ram_in_mib,
                resources.free_ram_in_mib,
            ),
        )));
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
        return Err(KubernetesError::Deploy(ServiceError::NotEnoughResources(
            format!(
                "There is not enough free Pods ({} required) on the Kubernetes cluster '{}'. \
                Consider to add one more node or upgrade your nodes configuration.",
                required_pods,
                kubernetes.name(),
            ),
        )));
    }

    Ok(())
}
