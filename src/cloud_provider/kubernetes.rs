use std::any::Any;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::thread;

use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::OperationResult;
use serde::{Deserialize, Serialize};

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service::CheckAction;
use crate::cloud_provider::utilities::{get_version_number, VersionsNumber};
use crate::cloud_provider::{service, CloudProvider, DeploymentTarget};
use crate::cmd::kubectl;
use crate::cmd::kubectl::{
    kubectl_delete_objects_in_all_namespaces, kubectl_exec_count_all_objects, kubectl_exec_get_node,
    kubectl_exec_version,
};
use crate::dns_provider::DnsProvider;
use crate::error::SimpleErrorKind::Other;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope, SimpleError, SimpleErrorKind,
};
use crate::models::{Context, Listen, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope, StringPath};
use crate::object_storage::ObjectStorage;
use crate::unit_conversion::{any_to_mi, cpu_string_to_float};

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

pub enum KubernetesUpgradeRequired {
    Masters,
    Workers,
}

/// Check if the current deployed version of Kubernetes requires an upgrade
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
/// * `wished_version` - kubernetes wished version
pub fn is_kubernetes_upgrade_required<P>(
    kubernetes_config: P,
    requested_version: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<KubernetesUpgradeRequired>, SimpleError>
where
    P: AsRef<Path>,
{
    let mut upgrade_required: Option<KubernetesUpgradeRequired> = None;
    let mut total_workers = 0;
    let mut non_up_to_date_workers = 0;

    let wished_version = match get_version_number(requested_version) {
        Ok(v) => v,
        Err(e) => {
            let msg = format!(
                "Don't know which Kubernetes version you want to support, upgrade is impossible. {:?}",
                e
            );
            error!("{}", &msg);
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(msg.to_string()),
            });
        }
    };

    // check master versions
    match get_kubernetes_masters_version(&kubernetes_config, envs.clone()) {
        Ok(deployed_version) => {
            match compare_kubernetes_cluster_versions_for_upgrade(&deployed_version, &wished_version) {
                Ok(x) if x => return Ok(Some(KubernetesUpgradeRequired::Masters)),
                Err(e) => return Err(e),
                _ => {}
            }
        }
        Err(e) => {
            let msg = format!("Can't get current deployed Kubernetes version. {:?}", e.message);
            error!("{}", &msg);
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(msg),
            });
        }
    }

    // check workers versions
    match get_kubernetes_workers_version(kubernetes_config, envs) {
        Ok(deployed_version) => {
            for node in deployed_version {
                total_workers += 1;
                match compare_kubernetes_cluster_versions_for_upgrade(&node, &wished_version) {
                    Ok(x) if x => {
                        upgrade_required = Some(KubernetesUpgradeRequired::Workers);
                        non_up_to_date_workers += 1;
                    }
                    Err(e) => return Err(e),
                    Ok(_) => {}
                }
            }
        }
        Err(e) => {
            let msg = format!("Can't get current deployed Kubernetes version. {:?}", e.message);
            error!("{}", &msg);
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(msg),
            });
        }
    }

    if upgrade_required.is_none() {
        info!("All workers are up to date, no upgrade required");
    } else {
        info!(
            "Kubernetes workers upgrade required, need to update {}/{} nodes",
            non_up_to_date_workers, total_workers
        );
    }

    Ok(upgrade_required)
}

/// Get kubernetes master nodes version
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
pub fn get_kubernetes_masters_version<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<VersionsNumber, SimpleError>
where
    P: AsRef<Path>,
{
    let v = kubectl_exec_version(kubernetes_config, envs)?;

    match get_version_number(format!("{}.{}", v.server_version.major, v.server_version.minor).as_str()) {
        Ok(vn) => Ok(vn),
        Err(e) => Err(SimpleError {
            kind: SimpleErrorKind::Other,
            message: Some(format!("Unable to determine Kubernetes master version. {}", e)),
        }),
    }
}

/// Get kubernetes workers nodes version (kubelet and kube-proxy)
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
pub fn get_kubernetes_workers_version<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<VersionsNumber>, SimpleError>
where
    P: AsRef<Path>,
{
    let mut nodes_versions: Vec<VersionsNumber> = vec![];
    let nodes = kubectl_exec_get_node(kubernetes_config, envs)?;

    for node in nodes.items {
        // check kubelet version
        match get_version_number(node.status.node_info.kubelet_version.as_str()) {
            Ok(vn) => nodes_versions.push(vn),
            Err(e) => {
                return Err(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some(format!(
                        "Unable to determine Kubernetes 'Kubelet' worker version. {}",
                        e
                    )),
                })
            }
        }
        // check kube-proxy version
        match get_version_number(node.status.node_info.kube_proxy_version.as_str()) {
            Ok(vn) => nodes_versions.push(vn),
            Err(e) => {
                return Err(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some(format!(
                        "Unable to determine Kubernetes 'Kube-proxy' worker version. {}",
                        e
                    )),
                })
            }
        }
    }

    Ok(nodes_versions)
}

fn compare_kubernetes_cluster_versions_for_upgrade(
    deployed_version: &VersionsNumber,
    wished_version: &VersionsNumber,
) -> Result<bool, SimpleError> {
    let mut upgrade_required = false;

    let deployed_minor_version = match &deployed_version.minor {
        Some(v) => v,
        None => {
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some("deployed kubernetes minor version was missing and is missing".to_string()),
            })
        }
    };

    let wished_minor_version = match &wished_version.minor {
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

    if &wished_minor_version > &deployed_minor_version {
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

#[cfg(test)]
mod tests {
    use crate::cloud_provider::kubernetes::compare_kubernetes_cluster_versions_for_upgrade;
    use crate::cloud_provider::utilities::{get_version_number, VersionsNumber};
    use crate::cmd::structs::{KubernetesList, KubernetesNode, KubernetesVersion};

    #[allow(dead_code)]
    pub fn print_kubernetes_version(provider_version: &VersionsNumber, provider: &VersionsNumber) {
        println!(
            "Provider version: {} | Wished version: {} | Is upgrade required: {:?}",
            provider_version.clone(),
            provider.clone(),
            compare_kubernetes_cluster_versions_for_upgrade(&provider_version, &provider).unwrap()
        )
    }

    #[test]
    pub fn check_kubernetes_master_versions() {
        struct KubernetesVersionToCheck {
            json: &'static str,
            wished_version: VersionsNumber,
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
                wished_version: VersionsNumber {
                    major: "1".to_string(),
                    minor: Some("16".to_string()),
                    patch: None,
                },
            },
            KubernetesVersionToCheck {
                json: kubectl_version_do,
                wished_version: VersionsNumber {
                    major: "1".to_string(),
                    minor: Some("18".to_string()),
                    patch: None,
                },
            },
        ];

        for mut provider in validate_providers {
            let provider_server_version: KubernetesVersion = serde_json::from_str(provider.json).unwrap();
            let provider_version = get_version_number(
                format!(
                    "{}",
                    VersionsNumber {
                        major: provider_server_version.server_version.major,
                        minor: Some(provider_server_version.server_version.minor),
                        patch: None
                    }
                )
                .as_str(),
            )
            .expect("wrong kubernetes cluster version");

            // upgrade is not required
            //print_kubernetes_version(&provider_version, &provider.wished_version);
            assert_eq!(
                compare_kubernetes_cluster_versions_for_upgrade(&provider_version, &provider.wished_version).unwrap(),
                false
            );

            // upgrade is required
            let add_one_version = provider.wished_version.minor.unwrap().parse::<i32>().unwrap() + 1;
            provider.wished_version.minor = Some(add_one_version.to_string());
            //print_kubernetes_version(&provider_version, &provider.wished_version);
            assert!(
                compare_kubernetes_cluster_versions_for_upgrade(&provider_version, &provider.wished_version).unwrap()
            )
        }
    }

    #[test]
    pub fn check_kubernetes_workers_versions() {
        struct KubernetesVersionToCheck {
            json: &'static str,
            wished_version: VersionsNumber,
        }

        let kubectl_version_aws = r#"
{
    "apiVersion": "v1",
    "items": [
        {
            "apiVersion": "v1",
            "kind": "Node",
            "metadata": {
                "annotations": {
                    "node.alpha.kubernetes.io/ttl": "0",
                    "volumes.kubernetes.io/controller-managed-attach-detach": "true"
                },
                "creationTimestamp": "2021-04-30T07:23:17Z",
                "labels": {
                    "beta.kubernetes.io/arch": "amd64",
                    "beta.kubernetes.io/instance-type": "t2.large",
                    "beta.kubernetes.io/os": "linux",
                    "eks.amazonaws.com/nodegroup": "qovery-dmubm9agk7sr8a8r-1",
                    "eks.amazonaws.com/nodegroup-image": "ami-0f8d6052f6e3a19d2",
                    "failure-domain.beta.kubernetes.io/region": "us-east-2",
                    "failure-domain.beta.kubernetes.io/zone": "us-east-2c",
                    "kubernetes.io/arch": "amd64",
                    "kubernetes.io/hostname": "ip-10-0-105-29.us-east-2.compute.internal",
                    "kubernetes.io/os": "linux"
                },
                "name": "ip-10-0-105-29.us-east-2.compute.internal",
                "resourceVersion": "76995588",
                "selfLink": "/api/v1/nodes/ip-10-0-105-29.us-east-2.compute.internal",
                "uid": "dbe8d9e1-481a-4de5-9fa5-1c0b2f2e94e9"
            },
            "spec": {
                "providerID": "aws:///us-east-2c/i-0a99d3bb7b27d62ac"
            },
            "status": {
                "addresses": [
                    {
                        "address": "10.0.105.29",
                        "type": "InternalIP"
                    },
                    {
                        "address": "3.139.58.222",
                        "type": "ExternalIP"
                    },
                    {
                        "address": "ip-10-0-105-29.us-east-2.compute.internal",
                        "type": "Hostname"
                    },
                    {
                        "address": "ip-10-0-105-29.us-east-2.compute.internal",
                        "type": "InternalDNS"
                    },
                    {
                        "address": "ec2-3-139-58-222.us-east-2.compute.amazonaws.com",
                        "type": "ExternalDNS"
                    }
                ],
                "allocatable": {
                    "attachable-volumes-aws-ebs": "39",
                    "cpu": "1930m",
                    "ephemeral-storage": "18242267924",
                    "hugepages-2Mi": "0",
                    "memory": "7408576Ki",
                    "pods": "35"
                },
                "capacity": {
                    "attachable-volumes-aws-ebs": "39",
                    "cpu": "2",
                    "ephemeral-storage": "20959212Ki",
                    "hugepages-2Mi": "0",
                    "memory": "8166336Ki",
                    "pods": "35"
                },
                "conditions": [
                    {
                        "lastHeartbeatTime": "2021-05-13T13:45:52Z",
                        "lastTransitionTime": "2021-04-30T07:23:16Z",
                        "message": "kubelet has sufficient memory available",
                        "reason": "KubeletHasSufficientMemory",
                        "status": "False",
                        "type": "MemoryPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T13:45:52Z",
                        "lastTransitionTime": "2021-04-30T07:23:16Z",
                        "message": "kubelet has no disk pressure",
                        "reason": "KubeletHasNoDiskPressure",
                        "status": "False",
                        "type": "DiskPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T13:45:52Z",
                        "lastTransitionTime": "2021-04-30T07:23:16Z",
                        "message": "kubelet has sufficient PID available",
                        "reason": "KubeletHasSufficientPID",
                        "status": "False",
                        "type": "PIDPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T13:45:52Z",
                        "lastTransitionTime": "2021-04-30T07:23:58Z",
                        "message": "kubelet is posting ready status",
                        "reason": "KubeletReady",
                        "status": "True",
                        "type": "Ready"
                    }
                ],
                "daemonEndpoints": {
                    "kubeletEndpoint": {
                        "Port": 10250
                    }
                },
                "images": [
                    {
                        "names": [
                            "grafana/loki@sha256:72fdf006e78141aa1f449acdbbaa195d4b7ad6be559a6710e4bcfe5ea2d7cc80",
                            "grafana/loki:1.6.0"
                        ],
                        "sizeBytes": 72825761
                    }
                ],
                "nodeInfo": {
                    "architecture": "amd64",
                    "bootID": "6707bff0-c846-4ae5-971f-6213a09cbb8d",
                    "containerRuntimeVersion": "docker://19.3.6",
                    "kernelVersion": "4.14.198-152.320.amzn2.x86_64",
                    "kubeProxyVersion": "v1.16.13-eks-ec92d4",
                    "kubeletVersion": "v1.16.13-eks-ec92d4",
                    "machineID": "9e41586f1a7b461a8987a1110da45b2a",
                    "operatingSystem": "linux",
                    "osImage": "Amazon Linux 2",
                    "systemUUID": "EC2E8B4C-92F9-213B-09B5-C0CD11A7EEB7"
                }
            }
        } 
    ],
    "kind": "List",
    "metadata": {
        "resourceVersion": "",
        "selfLink": ""
    }
}
"#;
        let kubectl_version_do = r#"
{
    "apiVersion": "v1",
    "items": [
        {
            "apiVersion": "v1",
            "kind": "Node",
            "metadata": {
                "annotations": {
                    "alpha.kubernetes.io/provided-node-ip": "10.1.2.12",
                    "csi.volume.kubernetes.io/nodeid": "{\"dobs.csi.digitalocean.com\":\"245738308\"}",
                    "io.cilium.network.ipv4-cilium-host": "10.244.60.81",
                    "io.cilium.network.ipv4-health-ip": "10.244.60.6",
                    "io.cilium.network.ipv4-pod-cidr": "10.244.60.0/25",
                    "node.alpha.kubernetes.io/ttl": "15",
                    "volumes.kubernetes.io/controller-managed-attach-detach": "true"
                },
                "creationTimestamp": "2021-05-12T08:22:46Z",
                "labels": {
                    "beta.kubernetes.io/arch": "amd64",
                    "beta.kubernetes.io/instance-type": "g-16vcpu-64gb",
                    "beta.kubernetes.io/os": "linux",
                    "doks.digitalocean.com/node-id": "2407f0b3-de84-4c26-b835-d979e1d5e873",
                    "doks.digitalocean.com/node-pool": "pool-un5t2n2gp",
                    "doks.digitalocean.com/node-pool-id": "5a2ea5fb-f826-4df1-b405-8b6f8d594098",
                    "doks.digitalocean.com/version": "1.18.10-do.2",
                    "failure-domain.beta.kubernetes.io/region": "nyc3",
                    "kubernetes.io/arch": "amd64",
                    "kubernetes.io/hostname": "pool-un5t2n2gp-8b870",
                    "kubernetes.io/os": "linux",
                    "node.kubernetes.io/instance-type": "g-16vcpu-64gb",
                    "region": "nyc3",
                    "topology.kubernetes.io/region": "nyc3"
                },
                "name": "pool-un5t2n2gp-8b870",
                "resourceVersion": "127317441",
                "selfLink": "/api/v1/nodes/pool-un5t2n2gp-8b870",
                "uid": "b75f6082-c597-44fa-ab88-16cf193f639b"
            },
            "spec": {
                "podCIDR": "10.244.60.0/25",
                "podCIDRs": [
                    "10.244.60.0/25"
                ],
                "providerID": "digitalocean://245738308"
            },
            "status": {
                "addresses": [
                    {
                        "address": "pool-un5t2n2gp-8b870",
                        "type": "Hostname"
                    },
                    {
                        "address": "10.1.2.12",
                        "type": "InternalIP"
                    },
                    {
                        "address": "167.99.121.123",
                        "type": "ExternalIP"
                    }
                ],
                "allocatable": {
                    "cpu": "16",
                    "ephemeral-storage": "190207346374",
                    "hugepages-1Gi": "0",
                    "hugepages-2Mi": "0",
                    "memory": "59942Mi",
                    "pods": "110"
                },
                "capacity": {
                    "cpu": "16",
                    "ephemeral-storage": "206388180Ki",
                    "hugepages-1Gi": "0",
                    "hugepages-2Mi": "0",
                    "memory": "65970528Ki",
                    "pods": "110"
                },
                "conditions": [
                    {
                        "lastHeartbeatTime": "2021-05-12T08:22:55Z",
                        "lastTransitionTime": "2021-05-12T08:22:55Z",
                        "message": "Cilium is running on this node",
                        "reason": "CiliumIsUp",
                        "status": "False",
                        "type": "NetworkUnavailable"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T14:33:56Z",
                        "lastTransitionTime": "2021-05-12T08:22:45Z",
                        "message": "kubelet has sufficient memory available",
                        "reason": "KubeletHasSufficientMemory",
                        "status": "False",
                        "type": "MemoryPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T14:33:56Z",
                        "lastTransitionTime": "2021-05-12T08:22:45Z",
                        "message": "kubelet has no disk pressure",
                        "reason": "KubeletHasNoDiskPressure",
                        "status": "False",
                        "type": "DiskPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T14:33:56Z",
                        "lastTransitionTime": "2021-05-12T08:22:45Z",
                        "message": "kubelet has sufficient PID available",
                        "reason": "KubeletHasSufficientPID",
                        "status": "False",
                        "type": "PIDPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T14:33:56Z",
                        "lastTransitionTime": "2021-05-12T08:22:56Z",
                        "message": "kubelet is posting ready status. AppArmor enabled",
                        "reason": "KubeletReady",
                        "status": "True",
                        "type": "Ready"
                    }
                ],
                "daemonEndpoints": {
                    "kubeletEndpoint": {
                        "Port": 10250
                    }
                },
                "images": [
                    {
                        "names": [
                            "digitalocean/doks-debug@sha256:d1a215845d868d1d6b2a6a93cb225a892d61e131954d71b5ef45664d77d8d2c7",
                            "digitalocean/doks-debug:latest"
                        ],
                        "sizeBytes": 752144177
                    }
                ],
                "nodeInfo": {
                    "architecture": "amd64",
                    "bootID": "917eead4-1db5-4709-9d28-01c3e469131a",
                    "containerRuntimeVersion": "docker://18.9.9",
                    "kernelVersion": "4.19.0-11-amd64",
                    "kubeProxyVersion": "v1.18.10",
                    "kubeletVersion": "v1.18.10",
                    "machineID": "503195a66f1a4417bfa02fc696aa3436",
                    "operatingSystem": "linux",
                    "osImage": "Debian GNU/Linux 10 (buster)",
                    "systemUUID": "503195a6-6f1a-4417-bfa0-2fc696aa3436"
                },
                "volumesAttached": [
                    {
                        "devicePath": "",
                        "name": "kubernetes.io/csi/dobs.csi.digitalocean.com^ba8713ff-b34a-11eb-9a5b-0a58ac146bd9"
                    }
                ],
                "volumesInUse": [
                    "kubernetes.io/csi/dobs.csi.digitalocean.com^ba8713ff-b34a-11eb-9a5b-0a58ac146bd9"
                ]
            }
        }
    ],
    "kind": "List",
    "metadata": {
        "resourceVersion": "",
        "selfLink": ""
    }
}
"#;

        let validate_providers = vec![
            KubernetesVersionToCheck {
                json: kubectl_version_aws,
                wished_version: VersionsNumber {
                    major: "1".to_string(),
                    minor: Some("16".to_string()),
                    patch: None,
                },
            },
            KubernetesVersionToCheck {
                json: kubectl_version_do,
                wished_version: VersionsNumber {
                    major: "1".to_string(),
                    minor: Some("18".to_string()),
                    patch: None,
                },
            },
        ];

        for mut provider in validate_providers {
            let provider_server_version: KubernetesList<KubernetesNode> =
                serde_json::from_str(provider.json).expect("Can't read workers json from {} provider");
            for node in provider_server_version.items {
                let kubelet = get_version_number(&node.status.node_info.kubelet_version).unwrap();
                let kube_proxy = get_version_number(&node.status.node_info.kube_proxy_version).unwrap();

                // upgrade is not required
                //print_kubernetes_version(&provider_version, &provider.wished_version);
                assert_eq!(
                    compare_kubernetes_cluster_versions_for_upgrade(&kubelet, &provider.wished_version).unwrap(),
                    false
                );
                assert_eq!(
                    compare_kubernetes_cluster_versions_for_upgrade(&kube_proxy, &provider.wished_version).unwrap(),
                    false
                );

                // upgrade is required
                let kubelet_add_one_version =
                    provider.wished_version.minor.clone().unwrap().parse::<i32>().unwrap() + 1;
                provider.wished_version.minor = Some(kubelet_add_one_version.to_string());
                //print_kubernetes_version(&provider_version, &provider.wished_version);
                assert!(compare_kubernetes_cluster_versions_for_upgrade(&kubelet, &provider.wished_version).unwrap());
            }
        }
    }
}
