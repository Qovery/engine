use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service::{Service, ServiceError};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd::CmdError;
use crate::models::{Context, ProgressListener};
use crate::transaction::CommitError;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::process::ExitStatus;
use std::rc::Rc;

pub trait Kubernetes {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn region(&self) -> &str;
    fn cloud_provider(&self) -> &dyn CloudProvider;
    fn is_valid(&self) -> Result<(), KubernetesError>;
    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>);
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
            KubernetesError::Deploy(e) |
            KubernetesError::Pause(e) |
            KubernetesError::Delete(e) => { Option::from(e) }
            _ => {
                None
            }
        };
    }
}
