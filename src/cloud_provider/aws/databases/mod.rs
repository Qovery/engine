use crate::cloud_provider::common::kubernetes::get_stateless_resource_information_for_user;
use crate::cloud_provider::service::Service;
use crate::cloud_provider::DeploymentTarget;
pub use mongodb::MongoDB;
pub use mysql::MySQL;
pub use postgresql::PostgreSQL;
pub use redis::Redis;

mod mongodb;
mod mysql;
mod postgresql;
mod redis;
mod utilities;

pub fn debug_logs(service: &dyn Service, deployment_target: &DeploymentTarget) -> Vec<String> {
    match deployment_target {
        DeploymentTarget::ManagedServices(_, _) => Vec::new(),
        DeploymentTarget::SelfHosted(kubernetes, environment) => {
            match get_stateless_resource_information_for_user(*kubernetes, *environment, service) {
                Ok(lines) => lines,
                Err(err) => {
                    error!(
                        "error while retrieving debug logs from database {}; error: {:?}",
                        service.name(),
                        err
                    );
                    Vec::new()
                }
            }
        }
    }
}
