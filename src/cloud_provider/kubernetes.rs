use std::any::Any;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::thread;

use serde::{Deserialize, Serialize};

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service::CheckAction;
use crate::cloud_provider::{service, CloudProvider, DeploymentTarget};
use crate::cmd::kubectl;
use crate::cmd::kubectl::{
    get_kubernetes_master_version, kubectl_delete_objects_in_all_namespaces, kubectl_exec_count_all_objects,
};
use crate::dns_provider::DnsProvider;
use crate::error::SimpleErrorKind::Other;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope, SimpleError, SimpleErrorKind,
};
use crate::models::{Context, Listen, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope, StringPath};
use crate::object_storage::ObjectStorage;
use crate::unit_conversion::{any_to_mi, cpu_string_to_float};
use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::OperationResult;
use std::path::Path;

pub trait Kubernetes: Listen {
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
    fn config_file_store(&self) -> &dyn ObjectStorage;
    fn is_valid(&self) -> Result<(), EngineError>;
    fn config_file(&self) -> Result<(StringPath, File), EngineError> {
        let bucket_name = format!("qovery-kubeconfigs-{}", self.id());
        let object_key = format!("{}.yaml", self.id());

        let (string_path, file) = self
            .config_file_store()
            .get(bucket_name.as_str(), object_key.as_str(), true)?;

        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                return Err(self.engine_error(EngineErrorCause::Internal, format!("{:?}", err)));
            }
        };

        let mut permissions = metadata.permissions();
        permissions.set_mode(0o400);
        let _ = std::fs::set_permissions(string_path.as_str(), permissions);

        Ok((string_path, file))
    }
    fn config_file_path(&self) -> Result<String, EngineError> {
        let (path, _) = self.config_file()?;
        Ok(path)
    }
    fn resources(&self, _environment: &Environment) -> Result<Resources, EngineError> {
        let kubernetes_config_file_path = self.config_file_path()?;

        let nodes = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context().execution_id(),
            crate::cmd::kubectl::kubectl_exec_get_node(
                kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
            ),
        )?;

        let mut resources = Resources {
            free_cpu: 0.0,
            max_cpu: 0.0,
            free_ram_in_mib: 0,
            max_ram_in_mib: 0,
            free_pods: 0,
            max_pods: 0,
            running_nodes: 0,
        };

        for node in nodes.items {
            resources.free_cpu += cpu_string_to_float(node.status.allocatable.cpu);
            resources.max_cpu += cpu_string_to_float(node.status.capacity.cpu);
            resources.free_ram_in_mib += any_to_mi(node.status.allocatable.memory);
            resources.max_ram_in_mib += any_to_mi(node.status.capacity.memory);
            resources.free_pods = match node.status.allocatable.pods.parse::<u16>() {
                Ok(v) => v,
                _ => 0,
            };
            resources.max_pods = match node.status.capacity.pods.parse::<u16>() {
                Ok(v) => v,
                _ => 0,
            };
            resources.running_nodes += 1;
        }

        Ok(resources)
    }
    fn on_create(&self) -> Result<(), EngineError>;
    fn on_create_error(&self) -> Result<(), EngineError>;
    fn on_upgrade(&self) -> Result<(), EngineError>;
    fn on_upgrade_error(&self) -> Result<(), EngineError>;
    fn on_downgrade(&self) -> Result<(), EngineError>;
    fn on_downgrade_error(&self) -> Result<(), EngineError>;
    fn on_pause(&self) -> Result<(), EngineError>;
    fn on_pause_error(&self) -> Result<(), EngineError>;
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
    fn instance_type(&self) -> &str;
    fn as_any(&self) -> &dyn Any;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Eks,
    Doks,
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

/// common function to deploy a complete environment through Kubernetes and the different
/// managed services.
pub fn deploy_environment(kubernetes: &dyn Kubernetes, environment: &Environment) -> Result<(), EngineError> {
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    let stateful_deployment_target = match kubernetes.kind() {
        Kind::Eks => match environment.kind {
            crate::cloud_provider::environment::Kind::Production => {
                DeploymentTarget::ManagedServices(kubernetes, environment)
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(kubernetes, environment)
            }
        },
        // FIXME: We don't have any managed service on DO for now
        Kind::Doks => DeploymentTarget::SelfHosted(kubernetes, environment),
    };

    // do not deploy if there is not enough resources
    let _ = check_kubernetes_has_enough_resources_to_deploy_environment(kubernetes, environment)?;

    // create all stateful services (database)
    for service in &environment.stateful_services {
        let _ = service::check_kubernetes_service_error(
            service.exec_action(&stateful_deployment_target),
            kubernetes,
            service,
            &stateful_deployment_target,
            &listeners_helper,
            "deployment",
            CheckAction::Deploy,
        )?;
        // check all deployed services
        for service in &environment.stateful_services {
            let _ = service::check_kubernetes_service_error(
                service.on_create_check(),
                kubernetes,
                service,
                &stateful_deployment_target,
                &listeners_helper,
                "check deployment",
                CheckAction::Deploy,
            )?;
        }
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
    let stateless_deployment_target = DeploymentTarget::SelfHosted(kubernetes, environment);

    // create all stateless services (router, application...)
    for service in &environment.stateless_services {
        let _ = service::check_kubernetes_service_error(
            service.exec_action(&stateless_deployment_target),
            kubernetes,
            service,
            &stateless_deployment_target,
            &listeners_helper,
            "deployment",
            CheckAction::Deploy,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // check all deployed services
    for service in &environment.stateful_services {
        let _ = service::check_kubernetes_service_error(
            service.on_create_check(),
            kubernetes,
            service,
            &stateful_deployment_target,
            &listeners_helper,
            "check deployment",
            CheckAction::Deploy,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    for service in &environment.stateless_services {
        let _ = service::check_kubernetes_service_error(
            service.on_create_check(),
            kubernetes,
            service,
            &stateless_deployment_target,
            &listeners_helper,
            "check deployment",
            CheckAction::Deploy,
        )?;
    }

    Ok(())
}

/// common function to react to an error when a environment deployment goes wrong
pub fn deploy_environment_error(kubernetes: &dyn Kubernetes, environment: &Environment) -> Result<(), EngineError> {
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    listeners_helper.deployment_in_progress(ProgressInfo::new(
        ProgressScope::Environment {
            id: kubernetes.context().execution_id().to_string(),
        },
        ProgressLevel::Warn,
        Some("An error occurred while trying to deploy the environment, so let's revert changes"),
        kubernetes.context().execution_id(),
    ));

    let stateful_deployment_target = match environment.kind {
        crate::cloud_provider::environment::Kind::Production => {
            DeploymentTarget::ManagedServices(kubernetes, environment)
        }
        crate::cloud_provider::environment::Kind::Development => DeploymentTarget::SelfHosted(kubernetes, environment),
    };

    // clean up all stateful services (database)
    for service in &environment.stateful_services {
        let _ = service::check_kubernetes_service_error(
            service.on_create_error(&stateful_deployment_target),
            kubernetes,
            service,
            &stateful_deployment_target,
            &listeners_helper,
            "revert deployment",
            CheckAction::Deploy,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
    let stateless_deployment_target = DeploymentTarget::SelfHosted(kubernetes, environment);

    // clean up all stateless services (router, application...)
    for service in &environment.stateless_services {
        let _ = service::check_kubernetes_service_error(
            service.on_create_error(&stateless_deployment_target),
            kubernetes,
            service,
            &stateless_deployment_target,
            &listeners_helper,
            "revert deployment",
            CheckAction::Deploy,
        )?;
    }

    Ok(())
}

/// common kubernetes function to pause a complete environment
pub fn pause_environment(kubernetes: &dyn Kubernetes, environment: &Environment) -> Result<(), EngineError> {
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    let stateful_deployment_target = match environment.kind {
        crate::cloud_provider::environment::Kind::Production => {
            DeploymentTarget::ManagedServices(kubernetes, environment)
        }
        crate::cloud_provider::environment::Kind::Development => DeploymentTarget::SelfHosted(kubernetes, environment),
    };

    // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
    let stateless_deployment_target = DeploymentTarget::SelfHosted(kubernetes, environment);

    // create all stateless services (router, application...)
    for service in &environment.stateless_services {
        let _ = service::check_kubernetes_service_error(
            service.on_pause(&stateless_deployment_target),
            kubernetes,
            service,
            &stateless_deployment_target,
            &listeners_helper,
            "pause",
            CheckAction::Pause,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // create all stateful services (database)
    for service in &environment.stateful_services {
        let _ = service::check_kubernetes_service_error(
            service.on_pause(&stateful_deployment_target),
            kubernetes,
            service,
            &stateful_deployment_target,
            &listeners_helper,
            "pause",
            CheckAction::Pause,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    for service in &environment.stateless_services {
        let _ = service::check_kubernetes_service_error(
            service.on_pause_check(),
            kubernetes,
            service,
            &stateless_deployment_target,
            &listeners_helper,
            "check pause",
            CheckAction::Pause,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // check all deployed services
    for service in &environment.stateful_services {
        let _ = service::check_kubernetes_service_error(
            service.on_pause_check(),
            kubernetes,
            service,
            &stateful_deployment_target,
            &listeners_helper,
            "check pause",
            CheckAction::Pause,
        )?;
    }

    Ok(())
}

/// common kubernetes function to delete a complete environment
pub fn delete_environment(kubernetes: &dyn Kubernetes, environment: &Environment) -> Result<(), EngineError> {
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    let stateful_deployment_target = match environment.kind {
        crate::cloud_provider::environment::Kind::Production => {
            DeploymentTarget::ManagedServices(kubernetes, environment)
        }
        crate::cloud_provider::environment::Kind::Development => DeploymentTarget::SelfHosted(kubernetes, environment),
    };

    // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
    let stateless_deployment_target = DeploymentTarget::SelfHosted(kubernetes, environment);

    // delete all stateless services (router, application...)
    for service in &environment.stateless_services {
        let _ = service::check_kubernetes_service_error(
            service.on_delete(&stateful_deployment_target),
            kubernetes,
            service,
            &stateless_deployment_target,
            &listeners_helper,
            "delete",
            CheckAction::Delete,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // delete all stateful services (database)
    for service in &environment.stateful_services {
        let _ = service::check_kubernetes_service_error(
            service.on_delete(&stateful_deployment_target),
            kubernetes,
            service,
            &stateful_deployment_target,
            &listeners_helper,
            "delete",
            CheckAction::Delete,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    for service in &environment.stateless_services {
        let _ = service::check_kubernetes_service_error(
            service.on_delete_check(),
            kubernetes,
            service,
            &stateless_deployment_target,
            &listeners_helper,
            "delete check",
            CheckAction::Delete,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // check all deployed services
    for service in &environment.stateful_services {
        let _ = service::check_kubernetes_service_error(
            service.on_delete_check(),
            kubernetes,
            service,
            &stateful_deployment_target,
            &listeners_helper,
            "delete check",
            CheckAction::Delete,
        )?;
    }

    // do not catch potential error - to confirm
    let _ = kubectl::kubectl_exec_delete_namespace(
        kubernetes.config_file_path()?,
        &environment.namespace(),
        kubernetes.cloud_provider().credentials_environment_variables(),
    );

    Ok(())
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

    if required_resources.cpu > resources.free_cpu && required_resources.ram_in_mib > resources.free_ram_in_mib {
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

    if required_resources.pods > resources.free_pods {
        // not enough free pods on the cluster
        let message = format!(
            "There is not enough free Pods ({} required) on the Kubernetes cluster '{}'. \
                Consider to add one more node or upgrade your nodes configuration.",
            required_resources.pods,
            kubernetes.name(),
        );

        return Err(kubernetes.engine_error(cause, message));
    }

    Ok(())
}

pub fn uninstall_cert_manager<P>(kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    // https://cert-manager.io/docs/installation/uninstall/kubernetes/
    info!("Delete cert-manager related objects to prepare deletion");

    let cert_manager_objects = vec![
        "Issuers",
        "ClusterIssuers",
        "Certificates",
        "CertificateRequests",
        "Orders",
        "Challenges",
    ];

    for object in cert_manager_objects {
        // check resource exist first
        match kubectl_exec_count_all_objects(&kubernetes_config, object, envs.clone()) {
            Ok(x) if x == 0 => continue,
            Err(e) => {
                warn!(
                    "encountering issues while trying to get objects kind {}: {:?}",
                    object, e.message
                );
                continue;
            }
            _ => {}
        }

        // delete if resource exists
        let result =
            retry::retry(
                Fibonacci::from_millis(5000).take(3),
                || match kubectl_delete_objects_in_all_namespaces(&kubernetes_config, object, envs.clone()) {
                    Ok(_) => OperationResult::Ok(()),
                    Err(e) => {
                        warn!("Failed to delete all {} objects, retrying...", object);
                        OperationResult::Retry(e)
                    }
                },
            );

        match result {
            Ok(_) => {}
            Err(Operation { error, .. }) => return Err(error),
            Err(retry::Error::Internal(msg)) => {
                let error_message = format!(
                    "Wasn't able to delete all objects type {}, it's a blocker to then delete cert-manager namespace. {}",
                    object,
                    format!("{:?}", msg)
                );
                return Err(SimpleError::new(Other, Some(error_message)));
            }
        };
    }

    Ok(())
}

/// Check if the current deployed version of Kubernetes requires an upgrade
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
/// * `wished_version` - kubernetes wished version
pub fn is_kubernetes_upgrade_required<P>(kubernetes_config: P, current_version: &str) -> Result<bool, SimpleError>
where
    P: AsRef<Path>,
{
    {
        let wished_version = match get_kubernetes_master_version(current_version) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!(
                    "Don't know which Kubernetes version you want to support, upgrade is impossible. {:?}",
                    e.message
                );
                error!("{}", &msg);
                return Err(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some(msg.to_string()),
                });
            }
        };

        match get_kubernetes_master_version(kubernetes_config) {
            Ok(deployed_version) => {
                let mut upgrade_required = false;

                let deployed_minor_version = match deployed_version.minor {
                    Some(v) => v,
                    None => {
                        return Err(SimpleError {
                            kind: SimpleErrorKind::Other,
                            message: Some("deployed kubernetes minor version was missing and is missing".to_string()),
                        })
                    }
                };

                let wished_minor_version = match wished_version.minor {
                    Some(v) => v,
                    None => {
                        return Err(SimpleError {
                            kind: SimpleErrorKind::Other,
                            message: Some("wished kubernetes minor version was expected and is missing".to_string()),
                        })
                    }
                };

                if wished_version.major > deployed_version.major {
                    info!("Kubernetes major version change detected");
                    upgrade_required = true;
                }

                if &deployed_minor_version > &wished_minor_version {
                    info!("Kubernetes minor version change detected");
                    upgrade_required = true;
                }

                if upgrade_required {
                    let old = format!("{}.{}", deployed_version.major, deployed_minor_version);
                    let new = format!("{}.{}", wished_version.major, wished_minor_version);
                    info!("Kubernetes cluster upgrade is required {} -> {}!!!", old, new);
                    return Ok(true);
                }

                info!("Kubernetes cluster upgrade is not required");
                Ok(false)
            }
            Err(e) => {
                let msg = format!("Can't get current deployed Kubernetes version. {:?}", e.message);
                error!("{}", &msg);
                Err(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some(msg),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::utilities::get_version_number;
    use crate::cmd::structs::KubernetesVersion;

    #[test]
    pub fn check_kubernetes_master_versions() {
        struct KubernetesVersionToCheck {
            json: &'static str,
            expected_version: String,
        }

        let kubectl_version_aws = r#"
{
  "clientVersion": {
    "major": "1",
    "minor": "21",
    "gitVersion": "v1.21.0",
    "gitCommit": "cb303e613a121a29364f75cc67d3d580833a7479",
    "gitTreeState": "archive",
    "buildDate": "2021-04-09T16:47:30Z",
    "goVersion": "go1.16.3",
    "compiler": "gc",
    "platform": "linux/amd64"
  },
  "serverVersion": {
    "major": "1",
    "minor": "16+",
    "gitVersion": "v1.16.15-eks-ad4801",
    "gitCommit": "ad4801fd44fe0f125c8d13f1b1d4827e8884476d",
    "gitTreeState": "clean",
    "buildDate": "2020-10-20T23:27:12Z",
    "goVersion": "go1.13.15",
    "compiler": "gc",
    "platform": "linux/amd64"
  }
}
"#;
        let kubectl_version_do = r#"
        {
  "clientVersion": {
    "major": "1",
    "minor": "21",
    "gitVersion": "v1.21.0",
    "gitCommit": "cb303e613a121a29364f75cc67d3d580833a7479",
    "gitTreeState": "archive",
    "buildDate": "2021-04-09T16:47:30Z",
    "goVersion": "go1.16.3",
    "compiler": "gc",
    "platform": "linux/amd64"
  },
  "serverVersion": {
    "major": "1",
    "minor": "18",
    "gitVersion": "v1.18.10",
    "gitCommit": "62876fc6d93e891aa7fbe19771e6a6c03773b0f7",
    "gitTreeState": "clean",
    "buildDate": "2020-10-15T01:43:56Z",
    "goVersion": "go1.13.15",
    "compiler": "gc",
    "platform": "linux/amd64"
  }
}
"#;

        let validate_providers = vec![
            KubernetesVersionToCheck {
                json: kubectl_version_aws,
                expected_version: "1.16".to_string(),
            },
            KubernetesVersionToCheck {
                json: kubectl_version_do,
                expected_version: "1.18".to_string(),
            },
        ];

        for provider in validate_providers {
            let provider_version: KubernetesVersion = serde_json::from_str(provider.json).unwrap();
            let version = get_version_number(
                format!(
                    "{}.{}",
                    provider_version.server_version.major, provider_version.server_version.minor
                )
                .as_str(),
            )
            .unwrap();
            let final_version = format!("{}.{}", version.major, version.minor.unwrap());
            assert_eq!(final_version, provider.expected_version);
        }
    }
}
