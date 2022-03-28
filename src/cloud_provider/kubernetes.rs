use std::any::Any;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::Duration;

use retry::delay::{Fibonacci, Fixed};
use retry::Error::Operation;
use retry::OperationResult;
use serde::{Deserialize, Serialize};

use crate::cloud_provider::aws::regions::AwsZones;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::models::{CpuLimits, NodeGroups};
use crate::cloud_provider::service::CheckAction;
use crate::cloud_provider::utilities::VersionsNumber;
use crate::cloud_provider::{service, CloudProvider, DeploymentTarget};
use crate::cmd::kubectl;
use crate::cmd::kubectl::{
    kubectl_delete_objects_in_all_namespaces, kubectl_exec_count_all_objects, kubectl_exec_delete_pod,
    kubectl_exec_get_node, kubectl_exec_version, kubectl_get_crash_looping_pods, kubernetes_get_all_pdbs,
};
use crate::cmd::structs::KubernetesNodeCondition;
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, GeneralStep, InfrastructureStep, Stage, Transmitter};
use crate::fs::workspace_directory;
use crate::io_models::ProgressLevel::Info;
use crate::io_models::{
    Action, Context, Listen, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope, QoveryIdentifier, StringPath,
};
use crate::logger::Logger;
use crate::object_storage::ObjectStorage;
use crate::unit_conversion::{any_to_mi, cpu_string_to_float};

pub trait ProviderOptions {}

pub trait Kubernetes: Listen {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;

    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }

    fn cluster_name(&self) -> String {
        format!("qovery-{}", self.id())
    }

    fn version(&self) -> &str;
    fn region(&self) -> String;
    fn zone(&self) -> &str;
    fn aws_zones(&self) -> Option<Vec<AwsZones>>;
    fn cloud_provider(&self) -> &dyn CloudProvider;
    fn dns_provider(&self) -> &dyn DnsProvider;
    fn logger(&self) -> &dyn Logger;
    fn config_file_store(&self) -> &dyn ObjectStorage;
    fn is_valid(&self) -> Result<(), EngineError>;

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        let context = self.context();
        EventDetails::new(
            Some(self.cloud_provider().kind()),
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            Some(self.region()),
            stage,
            Transmitter::Kubernetes(self.id().to_string(), self.name().to_string()),
        )
    }

    fn get_kubeconfig_filename(&self) -> String {
        format!("{}.yaml", self.id())
    }

    fn get_kubeconfig_file(&self) -> Result<(String, File), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        let bucket_name = format!("qovery-kubeconfigs-{}", self.id());
        let object_key = self.get_kubeconfig_filename();
        let stage = Stage::General(GeneralStep::RetrieveClusterConfig);

        // check if kubeconfig locally exists
        let local_kubeconfig = match self.get_temp_dir(event_details) {
            Ok(x) => {
                let local_kubeconfig_folder_path = format!("{}/{}", &x, &bucket_name);
                let local_kubeconfig_generated = format!("{}/{}", &local_kubeconfig_folder_path, &object_key);
                if Path::new(&local_kubeconfig_generated).exists() {
                    match File::open(&local_kubeconfig_generated) {
                        Ok(_) => Some(local_kubeconfig_generated),
                        Err(err) => {
                            self.logger().log(EngineEvent::Debug(
                                self.get_event_details(stage.clone()),
                                EventMessage::new(
                                    err.to_string(),
                                    Some(format!("Error, couldn't open {} file", &local_kubeconfig_generated,)),
                                ),
                            ));
                            None
                        }
                    }
                } else {
                    None
                }
            }
            Err(_) => None,
        };

        // otherwise, try to get it from object storage
        let (string_path, file) = match local_kubeconfig {
            Some(local_kubeconfig_generated) => {
                let kubeconfig_file =
                    File::open(&local_kubeconfig_generated).expect("couldn't read kubeconfig file, but file exists");

                (StringPath::from(&local_kubeconfig_generated), kubeconfig_file)
            }
            None => {
                match self
                    .config_file_store()
                    .get(bucket_name.as_str(), object_key.as_str(), true)
                {
                    Ok((path, file)) => (path, file),
                    Err(err) => {
                        let error = EngineError::new_cannot_retrieve_cluster_config_file(
                            self.get_event_details(stage),
                            err.into(),
                        );
                        self.logger().log(EngineEvent::Error(error.clone(), None));
                        return Err(error);
                    }
                }
            }
        };

        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                let error = EngineError::new_cannot_retrieve_cluster_config_file(
                    self.get_event_details(stage),
                    CommandError::new_from_safe_message(format!("Error getting file metadata, error: {}", err,)),
                );
                self.logger().log(EngineEvent::Error(error.clone(), None));
                return Err(error);
            }
        };

        let mut permissions = metadata.permissions();
        permissions.set_mode(0o400);
        if let Err(err) = std::fs::set_permissions(string_path.as_str(), permissions) {
            let error = EngineError::new_cannot_retrieve_cluster_config_file(
                self.get_event_details(stage),
                CommandError::new_from_safe_message(format!("Error setting file permissions, error: {}", err,)),
            );
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(error);
        }

        Ok((string_path, file))
    }

    fn get_kubeconfig_file_path(&self) -> Result<String, EngineError> {
        let (path, _) = self.get_kubeconfig_file()?;
        Ok(path)
    }

    fn resources(&self, _environment: &Environment) -> Result<Resources, EngineError> {
        let kubernetes_config_file_path = self.get_kubeconfig_file_path()?;
        let stage = Stage::General(GeneralStep::RetrieveClusterResources);

        let nodes = match crate::cmd::kubectl::kubectl_exec_get_node(
            kubernetes_config_file_path,
            self.cloud_provider().credentials_environment_variables(),
        ) {
            Ok(k) => k,
            Err(err) => {
                let error = EngineError::new_cannot_get_cluster_nodes(
                    self.get_event_details(stage),
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get cluster nodes, error: {}",
                        err.message()
                    )),
                );

                self.logger().log(EngineEvent::Error(error.clone(), None));

                return Err(error);
            }
        };

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
            resources.free_pods = node.status.allocatable.pods.parse::<u32>().unwrap_or(0);
            resources.max_pods = node.status.capacity.pods.parse::<u32>().unwrap_or(0);
            resources.running_nodes += 1;
        }

        Ok(resources)
    }

    fn on_create(&self) -> Result<(), EngineError>;
    fn on_create_error(&self) -> Result<(), EngineError>;

    fn upgrade(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));

        let kubeconfig = match self.get_kubeconfig_file() {
            Ok((path, _)) => path,
            Err(e) => return Err(e),
        };

        match is_kubernetes_upgradable(
            kubeconfig.clone(),
            self.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
        ) {
            Err(e) => Err(e),
            Ok(..) => match is_kubernetes_upgrade_required(
                kubeconfig,
                self.version(),
                self.cloud_provider().credentials_environment_variables(),
                event_details,
                self.logger(),
            ) {
                Ok(x) => self.upgrade_with_status(x),
                Err(e) => Err(e),
            },
        }
    }

    fn check_workers_on_upgrade(&self, targeted_version: String) -> Result<(), CommandError>
    where
        Self: Sized,
    {
        send_progress_on_long_task(self, Action::Create, || {
            check_workers_upgrade_status(
                self.get_kubeconfig_file_path().expect("Unable to get Kubeconfig"),
                self.cloud_provider().credentials_environment_variables(),
                targeted_version.clone(),
            )
        })
    }

    fn check_workers_on_create(&self) -> Result<(), CommandError>
    where
        Self: Sized,
    {
        let kubeconfig = match self.get_kubeconfig_file() {
            Ok((path, _)) => path,
            Err(e) => return Err(CommandError::new(e.message(), None)),
        };

        send_progress_on_long_task(self, Action::Create, || {
            check_workers_status(&kubeconfig, self.cloud_provider().credentials_environment_variables())
        })
    }
    fn upgrade_with_status(&self, kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), EngineError>;
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

    fn send_to_customer(&self, message: &str, listeners_helper: &ListenersHelper) {
        listeners_helper.upgrade_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context().execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(message),
            self.context().execution_id(),
        ))
    }

    fn get_temp_dir(&self, event_details: EventDetails) -> Result<String, EngineError> {
        workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("bootstrap/{}", self.id()),
        )
        .map_err(|err| {
            EngineError::new_cannot_get_workspace_directory(event_details, CommandError::new(err.to_string(), None))
        })
    }

    fn delete_crashlooping_pods(
        &self,
        namespace: Option<&str>,
        selector: Option<&str>,
        restarted_min_count: Option<usize>,
        envs: Vec<(&str, &str)>,
        stage: Stage,
    ) -> Result<(), EngineError> {
        let event_details = self.get_event_details(stage);

        match self.get_kubeconfig_file() {
            Err(e) => return Err(e),
            Ok((config_path, _)) => match kubectl_get_crash_looping_pods(
                &config_path,
                namespace,
                selector,
                restarted_min_count,
                envs.clone(),
            ) {
                Ok(pods) => {
                    for pod in pods {
                        if let Err(e) = kubectl_exec_delete_pod(
                            &config_path,
                            pod.metadata.namespace.as_str(),
                            pod.metadata.name.as_str(),
                            envs.clone(),
                        ) {
                            return Err(EngineError::new_k8s_cannot_delete_pod(
                                event_details,
                                pod.metadata.name.to_string(),
                                e,
                            ));
                        }
                    }
                }
                Err(e) => {
                    return Err(EngineError::new_k8s_cannot_get_crash_looping_pods(event_details, e));
                }
            },
        };

        Ok(())
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
    ScwKapsule,
}

impl Display for Kind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Eks => "EKS",
            Kind::Doks => "DOKS",
            Kind::ScwKapsule => "ScwKapsule",
        })
    }
}

#[derive(Debug)]
pub struct Resources {
    pub free_cpu: f32,
    pub max_cpu: f32,
    pub free_ram_in_mib: u32,
    pub max_ram_in_mib: u32,
    pub free_pods: u32,
    pub max_pods: u32,
    pub running_nodes: u32,
}

/// common function to deploy a complete environment through Kubernetes and the different
/// managed services.
pub fn deploy_environment(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<(), EngineError> {
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    let stateful_deployment_target = match kubernetes.kind() {
        Kind::Eks => DeploymentTarget {
            kubernetes,
            environment,
        },
        Kind::Doks => DeploymentTarget {
            kubernetes,
            environment,
        },
        Kind::ScwKapsule => DeploymentTarget {
            kubernetes,
            environment,
        },
    };

    // create all stateful services (database)
    for service in environment.stateful_services() {
        let _ = service::check_kubernetes_service_error(
            service.exec_action(&stateful_deployment_target),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateful_deployment_target,
            &listeners_helper,
            "deployment",
            CheckAction::Deploy,
        )?;

        // check all deployed services
        let _ = service::check_kubernetes_service_error(
            service.exec_check_action(),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateful_deployment_target,
            &listeners_helper,
            "check deployment",
            CheckAction::Deploy,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
    let stateless_deployment_target = DeploymentTarget {
        kubernetes,
        environment,
    };

    // create all stateless services (router, application...)
    for service in environment.stateless_services() {
        let _ = service::check_kubernetes_service_error(
            service.exec_action(&stateless_deployment_target),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateless_deployment_target,
            &listeners_helper,
            "deployment",
            CheckAction::Deploy,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // check all deployed services
    for service in environment.stateful_services() {
        let _ = service::check_kubernetes_service_error(
            service.exec_check_action(),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateless_deployment_target,
            &listeners_helper,
            "check deployment",
            CheckAction::Deploy,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    for service in environment.stateless_services() {
        let _ = service::check_kubernetes_service_error(
            service.exec_check_action(),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateless_deployment_target,
            &listeners_helper,
            "check deployment",
            CheckAction::Deploy,
        )?;
    }

    Ok(())
}

/// common function to react to an error when a environment deployment goes wrong
pub fn deploy_environment_error(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<(), EngineError> {
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    listeners_helper.deployment_in_progress(ProgressInfo::new(
        ProgressScope::Environment {
            id: kubernetes.context().execution_id().to_string(),
        },
        ProgressLevel::Warn,
        Some("An error occurred while trying to deploy the environment, so let's revert changes"),
        kubernetes.context().execution_id(),
    ));

    let stateful_deployment_target = DeploymentTarget {
        kubernetes,
        environment,
    };

    // clean up all stateful services (database)
    for service in environment.stateful_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_create_error(&stateful_deployment_target),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateful_deployment_target,
            &listeners_helper,
            "revert deployment",
            CheckAction::Deploy,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
    let stateless_deployment_target = DeploymentTarget {
        kubernetes,
        environment,
    };

    // clean up all stateless services (router, application...)
    for service in environment.stateless_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_create_error(&stateless_deployment_target),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateless_deployment_target,
            &listeners_helper,
            "revert deployment",
            CheckAction::Deploy,
        )?;
    }

    Ok(())
}

/// common kubernetes function to pause a complete environment
pub fn pause_environment(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<(), EngineError> {
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    let stateful_deployment_target = DeploymentTarget {
        kubernetes,
        environment,
    };

    // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
    let stateless_deployment_target = DeploymentTarget {
        kubernetes,
        environment,
    };

    // create all stateless services (router, application...)
    for service in environment.stateless_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_pause(&stateless_deployment_target),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateless_deployment_target,
            &listeners_helper,
            "pause",
            CheckAction::Pause,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // create all stateful services (database)
    for service in environment.stateful_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_pause(&stateful_deployment_target),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateful_deployment_target,
            &listeners_helper,
            "pause",
            CheckAction::Pause,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    for service in environment.stateless_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_pause_check(),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateless_deployment_target,
            &listeners_helper,
            "check pause",
            CheckAction::Pause,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // check all deployed services
    for service in environment.stateful_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_pause_check(),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateful_deployment_target,
            &listeners_helper,
            "check pause",
            CheckAction::Pause,
        )?;
    }

    Ok(())
}

/// common kubernetes function to delete a complete environment
pub fn delete_environment(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<(), EngineError> {
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    let stateful_deployment_target = DeploymentTarget {
        kubernetes,
        environment,
    };

    // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
    let stateless_deployment_target = DeploymentTarget {
        kubernetes,
        environment,
    };

    // delete all stateless services (router, application...)
    for service in environment.stateless_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_delete(&stateful_deployment_target),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateless_deployment_target,
            &listeners_helper,
            "delete",
            CheckAction::Delete,
        );
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // delete all stateful services (database)
    for service in environment.stateful_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_delete(&stateful_deployment_target),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateful_deployment_target,
            &listeners_helper,
            "delete",
            CheckAction::Delete,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    for service in environment.stateless_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_delete_check(),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateless_deployment_target,
            &listeners_helper,
            "delete check",
            CheckAction::Delete,
        )?;
    }

    // Quick fix: adding 100 ms delay to avoid race condition on service status update
    thread::sleep(std::time::Duration::from_millis(100));

    // check all deployed services
    for service in environment.stateful_services() {
        let _ = service::check_kubernetes_service_error(
            service.on_delete_check(),
            kubernetes,
            service,
            event_details.clone(),
            logger,
            &stateful_deployment_target,
            &listeners_helper,
            "delete check",
            CheckAction::Delete,
        )?;
    }

    // do not catch potential error - to confirm
    let _ = kubectl::kubectl_exec_delete_namespace(
        kubernetes.get_kubeconfig_file_path()?,
        environment.namespace(),
        kubernetes.cloud_provider().credentials_environment_variables(),
    );

    Ok(())
}

pub fn uninstall_cert_manager<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<(), EngineError>
where
    P: AsRef<Path>,
{
    // https://cert-manager.io/docs/installation/uninstall/kubernetes/

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
        if let Err(e) = kubectl_exec_count_all_objects(&kubernetes_config, object, envs.clone()) {
            logger.log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(
                    format!(
                        "Encountering issues while trying to get objects kind {}: {:?}",
                        object,
                        e.message()
                    ),
                    None,
                ),
            ));
            continue;
        }

        // delete if resource exists
        match retry::retry(
            Fibonacci::from_millis(5000).take(3),
            || match kubectl_delete_objects_in_all_namespaces(&kubernetes_config, object, envs.clone()) {
                Ok(_) => OperationResult::Ok(()),
                Err(e) => {
                    logger.log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(format!("Failed to delete all {} objects, retrying...", object,), None),
                    ));
                    OperationResult::Retry(e)
                }
            },
        ) {
            Ok(_) => {}
            Err(Operation { error, .. }) => {
                return Err(EngineError::new_cannot_uninstall_helm_chart(
                    event_details,
                    "Cert-Manager".to_string(),
                    object.to_string(),
                    error,
                ))
            }
            Err(retry::Error::Internal(msg)) => {
                return Err(EngineError::new_cannot_uninstall_helm_chart(
                    event_details,
                    "Cert-Manager".to_string(),
                    object.to_string(),
                    CommandError::new_from_safe_message(msg),
                ))
            }
        }
    }

    Ok(())
}

pub fn is_kubernetes_upgrade_required<P>(
    kubernetes_config: P,
    requested_version: &str,
    envs: Vec<(&str, &str)>,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<KubernetesUpgradeStatus, EngineError>
where
    P: AsRef<Path>,
{
    // check master versions
    let v = match kubectl_exec_version(&kubernetes_config, envs.clone()) {
        Ok(v) => v,
        Err(e) => return Err(EngineError::new_cannot_execute_k8s_exec_version(event_details, e)),
    };
    let raw_version = format!("{}.{}", v.server_version.major, v.server_version.minor);
    let masters_version = match VersionsNumber::from_str(raw_version.as_str()) {
        Ok(vn) => vn,
        Err(_) => {
            return Err(EngineError::new_cannot_determine_k8s_master_version(
                event_details,
                raw_version.to_string(),
            ))
        }
    };

    // check workers versions
    let mut workers_version: Vec<VersionsNumber> = vec![];
    let nodes = match kubectl_exec_get_node(kubernetes_config, envs) {
        Ok(n) => n,
        Err(e) => return Err(EngineError::new_cannot_get_cluster_nodes(event_details, e)),
    };

    for node in nodes.items {
        // check kubelet version
        match VersionsNumber::from_str(node.status.node_info.kubelet_version.as_str()) {
            Ok(vn) => workers_version.push(vn),
            Err(_) => {
                return Err(EngineError::new_cannot_determine_k8s_kubelet_worker_version(
                    event_details,
                    node.status.node_info.kubelet_version.to_string(),
                ))
            }
        }

        // check kube-proxy version
        match VersionsNumber::from_str(node.status.node_info.kube_proxy_version.as_str()) {
            Ok(vn) => workers_version.push(vn),
            Err(_) => {
                return Err(EngineError::new_cannot_determine_k8s_kube_proxy_version(
                    event_details,
                    node.status.node_info.kube_proxy_version.to_string(),
                ))
            }
        }
    }

    check_kubernetes_upgrade_status(requested_version, masters_version, workers_version, event_details, logger)
}

pub fn is_kubernetes_upgradable<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    event_details: EventDetails,
) -> Result<(), EngineError>
where
    P: AsRef<Path>,
{
    match kubernetes_get_all_pdbs(kubernetes_config, envs, None) {
        Ok(pdbs) => match pdbs.items.is_some() {
            false => Ok(()),
            true => {
                for pdb in pdbs.items.unwrap() {
                    if pdb.status.current_healthy < pdb.status.desired_healthy {
                        return Err(EngineError::new_k8s_pod_disruption_budget_invalid_state(
                            event_details,
                            pdb.metadata.name,
                        ));
                    }
                }
                Ok(())
            }
        },
        Err(err) => Err(EngineError::new_k8s_cannot_retrieve_pods_disruption_budget(event_details, err)),
    }
}

pub fn check_workers_upgrade_status<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    target_version: String,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let result = retry::retry(Fixed::from_millis(10000).take(360), || {
        match kubectl_exec_get_node(kubernetes_config.as_ref(), envs.clone()) {
            Err(e) => OperationResult::Retry(e),
            Ok(nodes) => {
                for node in nodes.items.iter() {
                    if !node.status.node_info.kubelet_version.contains(&target_version[..4]) {
                        return OperationResult::Retry(CommandError::new_from_safe_message(
                            "There are still not upgraded nodes.".to_string(),
                        ));
                    }
                }
                OperationResult::Ok(())
            }
        }
    });

    return match result {
        Ok(_) => match check_workers_status(kubernetes_config.as_ref(), envs.clone()) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        },
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(CommandError::new_from_safe_message(e)),
    };
}

pub fn check_workers_status<P>(kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let result = retry::retry(Fixed::from_millis(10000).take(60), || {
        match kubectl_exec_get_node(kubernetes_config.as_ref(), envs.clone()) {
            Err(e) => OperationResult::Retry(e),
            Ok(nodes) => {
                let mut conditions: Vec<KubernetesNodeCondition> = Vec::new();
                for node in nodes.items.into_iter() {
                    conditions.extend(node.status.conditions.into_iter());
                }

                for condition in conditions.iter() {
                    if condition.condition_type == "Ready" && condition.status != "True" {
                        return OperationResult::Retry(CommandError::new_from_safe_message(
                            "There are still not ready worker nodes.".to_string(),
                        ));
                    }
                }
                OperationResult::Ok(())
            }
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(CommandError::new_from_safe_message(e)),
    }
}

#[derive(Debug, PartialEq)]
pub enum KubernetesNodesType {
    Masters,
    Workers,
}

#[derive(Debug)]
pub struct KubernetesUpgradeStatus {
    pub required_upgrade_on: Option<KubernetesNodesType>,
    pub requested_version: VersionsNumber,
    pub deployed_masters_version: VersionsNumber,
    pub deployed_workers_version: VersionsNumber,
    pub older_masters_version_detected: bool,
    pub older_workers_version_detected: bool,
}

/// Check if Kubernetes cluster elements are requiring an upgrade
///
/// It will gives useful info:
/// * versions of masters
/// * versions of workers (the oldest)
/// * which type of nodes should be upgraded in priority
/// * is the requested version is older than the current deployed
///
fn check_kubernetes_upgrade_status(
    requested_version: &str,
    deployed_masters_version: VersionsNumber,
    deployed_workers_version: Vec<VersionsNumber>,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<KubernetesUpgradeStatus, EngineError> {
    let mut total_workers = 0;
    let mut non_up_to_date_workers = 0;
    let mut required_upgrade_on = None;
    let mut older_masters_version_detected = false;
    let mut older_workers_version_detected = false;

    let wished_version = match VersionsNumber::from_str(requested_version) {
        Ok(v) => v,
        Err(e) => {
            return Err(EngineError::new_cannot_determine_k8s_requested_upgrade_version(
                event_details,
                requested_version.to_string(),
                Some(e),
            ));
        }
    };

    // check master versions
    match compare_kubernetes_cluster_versions_for_upgrade(&deployed_masters_version, &wished_version) {
        Ok(x) => {
            if let Some(msg) = x.message {
                logger.log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));
            };
            if x.older_version_detected {
                older_masters_version_detected = x.older_version_detected;
            }
            if x.upgraded_required {
                required_upgrade_on = Some(KubernetesNodesType::Masters);
            }
        }
        Err(e) => {
            return Err(
                EngineError::new_k8s_version_upgrade_deployed_vs_requested_versions_inconsistency(
                    event_details,
                    deployed_masters_version,
                    wished_version,
                    e,
                ),
            )
        }
    };

    // check workers versions
    if deployed_workers_version.is_empty() {
        logger.log(EngineEvent::Info(
            event_details,
            EventMessage::new_from_safe(
                "No worker nodes found, can't check if upgrade is required for workers".to_string(),
            ),
        ));

        return Ok(KubernetesUpgradeStatus {
            required_upgrade_on,
            requested_version: wished_version,
            deployed_masters_version: deployed_masters_version.clone(),
            deployed_workers_version: deployed_masters_version,
            older_masters_version_detected,
            older_workers_version_detected,
        });
    }

    let mut workers_oldest_version = deployed_workers_version[0].clone();

    for node in deployed_workers_version {
        total_workers += 1;
        match compare_kubernetes_cluster_versions_for_upgrade(&node, &wished_version) {
            Ok(x) => {
                if x.older_version_detected {
                    older_workers_version_detected = x.older_version_detected;
                    workers_oldest_version = node.clone();
                };
                if x.upgraded_required {
                    workers_oldest_version = node;
                    match required_upgrade_on {
                        Some(KubernetesNodesType::Masters) => {}
                        _ => required_upgrade_on = Some(KubernetesNodesType::Workers),
                    };
                };
                non_up_to_date_workers += 1;
            }
            Err(e) => {
                return Err(
                    EngineError::new_k8s_version_upgrade_deployed_vs_requested_versions_inconsistency(
                        event_details,
                        node,
                        wished_version,
                        e,
                    ),
                )
            }
        }
    }

    logger.log(EngineEvent::Info(
        event_details,
        EventMessage::new_from_safe(match &required_upgrade_on {
            None => "All workers are up to date, no upgrade required".to_string(),
            Some(node_type) => match node_type {
                KubernetesNodesType::Masters => "Kubernetes master upgrade required".to_string(),
                KubernetesNodesType::Workers => format!(
                    "Kubernetes workers upgrade required, need to update {}/{} nodes",
                    non_up_to_date_workers, total_workers
                ),
            },
        }),
    ));

    Ok(KubernetesUpgradeStatus {
        required_upgrade_on,
        requested_version: wished_version,
        deployed_masters_version,
        deployed_workers_version: workers_oldest_version,
        older_masters_version_detected,
        older_workers_version_detected,
    })
}

pub struct CompareKubernetesStatus {
    pub upgraded_required: bool,
    pub older_version_detected: bool,
    pub message: Option<String>,
}

pub fn compare_kubernetes_cluster_versions_for_upgrade(
    deployed_version: &VersionsNumber,
    wished_version: &VersionsNumber,
) -> Result<CompareKubernetesStatus, CommandError> {
    let mut messages: Vec<&str> = Vec::new();
    let mut upgrade_required = CompareKubernetesStatus {
        upgraded_required: false,
        older_version_detected: false,
        message: None,
    };

    let deployed_minor_version = match &deployed_version.minor {
        Some(v) => v,
        None => {
            return Err(CommandError::new_from_safe_message(
                "deployed kubernetes minor version was missing and is missing".to_string(),
            ))
        }
    };

    let wished_minor_version = match &wished_version.minor {
        Some(v) => v,
        None => {
            return Err(CommandError::new_from_safe_message(
                "wished kubernetes minor version was expected and is missing".to_string(),
            ))
        }
    };

    if wished_version.major > deployed_version.major {
        upgrade_required.upgraded_required = true;
        messages.push("Kubernetes major version change detected");
    }

    if wished_version.major < deployed_version.major {
        upgrade_required.upgraded_required = false;
        upgrade_required.older_version_detected = true;
        messages.push("Older Kubernetes major version detected");
    }

    if wished_minor_version > deployed_minor_version {
        upgrade_required.upgraded_required = true;
        messages.push("Kubernetes minor version change detected");
    }

    if wished_minor_version < deployed_minor_version {
        upgrade_required.upgraded_required = false;
        upgrade_required.older_version_detected = true;
        messages.push("Older Kubernetes minor version detected");
    }

    let mut final_message = "Kubernetes cluster upgrade is not required".to_string();
    if upgrade_required.upgraded_required {
        let old = format!("{}.{}", deployed_version.major, deployed_minor_version);
        let new = format!("{}.{}", wished_version.major, wished_minor_version);
        final_message = format!("Kubernetes cluster upgrade is required {} -> {} !!!", old, new);
    }
    messages.push(final_message.as_str());
    upgrade_required.message = Some(messages.join(". "));

    Ok(upgrade_required)
}

pub trait InstanceType {
    fn to_cloud_provider_format(&self) -> String;
}

impl NodeGroups {
    pub fn new(
        group_name: String,
        min_nodes: i32,
        max_nodes: i32,
        instance_type: String,
        disk_size_in_gib: i32,
    ) -> Result<Self, CommandError> {
        if min_nodes > max_nodes {
            return Err(CommandError::new_from_safe_message(format!(
                "The number of minimum nodes ({}) for group name {} is higher than maximum nodes ({})",
                &group_name, &min_nodes, &max_nodes
            )));
        }

        Ok(NodeGroups {
            name: group_name,
            id: None,
            min_nodes,
            max_nodes,
            instance_type,
            disk_size_in_gib,
        })
    }
}

/// TODO(benjaminch): to be refactored with similar function in services.rs
/// This function call (start|pause|delete)_in_progress function every 10 seconds when a
/// long blocking task is running.
pub fn send_progress_on_long_task<K, R, F>(kubernetes: &K, action: Action, long_task: F) -> R
where
    K: Kubernetes + Listen,
    F: Fn() -> R,
{
    let waiting_message = match action {
        Action::Create => Some(format!(
            "Infrastructure '{}' deployment is in progress...",
            kubernetes.name_with_id()
        )),
        Action::Pause => Some(format!(
            "Infrastructure '{}' pause is in progress...",
            kubernetes.name_with_id()
        )),
        Action::Delete => Some(format!(
            "Infrastructure '{}' deletion is in progress...",
            kubernetes.name_with_id()
        )),
        Action::Nothing => None,
    };

    send_progress_on_long_task_with_message(kubernetes, waiting_message, action, long_task)
}

/// TODO(benjaminch): to be refactored with similar function in services.rs
/// This function call (start|pause|delete)_in_progress function every 10 seconds when a
/// long blocking task is running.
pub fn send_progress_on_long_task_with_message<K, R, F>(
    kubernetes: &K,
    waiting_message: Option<String>,
    action: Action,
    long_task: F,
) -> R
where
    K: Kubernetes + Listen,
    F: Fn() -> R,
{
    let listeners = std::clone::Clone::clone(kubernetes.listeners());
    let logger = kubernetes.logger().clone_dyn();
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));

    let progress_info = ProgressInfo::new(
        ProgressScope::Infrastructure {
            execution_id: kubernetes.context().execution_id().to_string(),
        },
        Info,
        waiting_message.clone(),
        kubernetes.context().execution_id(),
    );

    let (tx, rx) = mpsc::channel();

    // monitor thread to notify user while the blocking task is executed
    let _ = std::thread::Builder::new()
        .name("task-monitor".to_string())
        .spawn(move || {
            // stop the thread when the blocking task is done
            let listeners_helper = ListenersHelper::new(&listeners);
            let action = action;
            let progress_info = progress_info;
            let waiting_message = waiting_message.unwrap_or_else(|| "no message ...".to_string());

            loop {
                // do notify users here
                let progress_info = std::clone::Clone::clone(&progress_info);
                let event_details = std::clone::Clone::clone(&event_details);
                let event_message = EventMessage::new_from_safe(waiting_message.to_string());

                match action {
                    Action::Create => {
                        listeners_helper.deployment_in_progress(progress_info);
                        logger.log(EngineEvent::Info(
                            EventDetails::clone_changing_stage(
                                event_details,
                                Stage::Infrastructure(InfrastructureStep::Create),
                            ),
                            event_message,
                        ));
                    }
                    Action::Pause => {
                        listeners_helper.pause_in_progress(progress_info);
                        logger.log(EngineEvent::Info(
                            EventDetails::clone_changing_stage(
                                event_details,
                                Stage::Infrastructure(InfrastructureStep::Pause),
                            ),
                            event_message,
                        ));
                    }
                    Action::Delete => {
                        listeners_helper.delete_in_progress(progress_info);
                        logger.log(EngineEvent::Info(
                            EventDetails::clone_changing_stage(
                                event_details,
                                Stage::Infrastructure(InfrastructureStep::Delete),
                            ),
                            event_message,
                        ));
                    }
                    Action::Nothing => {} // should not happens
                };

                thread::sleep(Duration::from_secs(10));

                // watch for thread termination
                match rx.try_recv() {
                    Ok(_) | Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {}
                }
            }
        });

    let blocking_task_result = long_task();
    let _ = tx.send(());

    blocking_task_result
}

pub fn validate_k8s_required_cpu_and_burstable(
    listener_helper: &ListenersHelper,
    execution_id: &str,
    context_id: &str,
    total_cpu: String,
    cpu_burst: String,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<CpuLimits, CommandError> {
    let total_cpu_float = convert_k8s_cpu_value_to_f32(total_cpu.clone())?;
    let cpu_burst_float = convert_k8s_cpu_value_to_f32(cpu_burst.clone())?;
    let mut set_cpu_burst = cpu_burst.clone();

    if cpu_burst_float < total_cpu_float {
        let message = format!(
            "CPU burst value '{}' was lower than the desired total of CPUs {}, using burstable value.",
            cpu_burst, total_cpu,
        );

        listener_helper.error(ProgressInfo::new(
            ProgressScope::Environment {
                id: execution_id.to_string(),
            },
            ProgressLevel::Warn,
            Some(message.to_string()),
            context_id,
        ));

        logger.log(EngineEvent::Warning(event_details, EventMessage::new_from_safe(message)));

        set_cpu_burst = total_cpu.clone();
    }

    Ok(CpuLimits {
        cpu_limit: set_cpu_burst,
        cpu_request: total_cpu,
    })
}

pub fn convert_k8s_cpu_value_to_f32(value: String) -> Result<f32, CommandError> {
    if value.ends_with('m') {
        let mut value_number_string = value;
        value_number_string.pop();
        return match value_number_string.parse::<f32>() {
            Ok(n) => {
                Ok(n * 0.001) // return in milli cpu the value
            }
            Err(e) => Err(CommandError::new(
                e.to_string(),
                Some(format!(
                    "Error while trying to parse `{}` to float 32.",
                    value_number_string.as_str()
                )),
            )),
        };
    }

    match value.parse::<f32>() {
        Ok(n) => Ok(n),
        Err(e) => Err(CommandError::new(
            e.to_string(),
            Some(format!("Error while trying to parse `{}` to float 32.", value.as_str())),
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::Kind::Aws;
    use std::str::FromStr;

    use crate::cloud_provider::kubernetes::{
        check_kubernetes_upgrade_status, compare_kubernetes_cluster_versions_for_upgrade, convert_k8s_cpu_value_to_f32,
        validate_k8s_required_cpu_and_burstable, KubernetesNodesType,
    };
    use crate::cloud_provider::models::CpuLimits;
    use crate::cloud_provider::utilities::VersionsNumber;
    use crate::cmd::structs::{KubernetesList, KubernetesNode, KubernetesVersion};
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::{ListenersHelper, QoveryIdentifier};
    use crate::logger::StdIoLogger;

    #[test]
    pub fn check_kubernetes_upgrade_method() {
        let version_1_16 = VersionsNumber::new("1".to_string(), Some("16".to_string()), None, None);
        let version_1_17 = VersionsNumber::new("1".to_string(), Some("17".to_string()), None, None);
        let event_details = EventDetails::new(
            None,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            None,
            Stage::Infrastructure(InfrastructureStep::Upgrade),
            Transmitter::Kubernetes(QoveryIdentifier::new_random().to_string(), "test".to_string()),
        );
        let logger = StdIoLogger::new();

        // test full cluster upgrade (masters + workers)
        let result = check_kubernetes_upgrade_status(
            "1.17",
            version_1_16.clone(),
            vec![version_1_16.clone()],
            event_details.clone(),
            &logger,
        )
        .unwrap();
        assert_eq!(result.required_upgrade_on.unwrap(), KubernetesNodesType::Masters); // master should be performed first
        assert_eq!(result.deployed_masters_version, version_1_16);
        assert_eq!(result.deployed_workers_version, version_1_16);
        assert_eq!(result.older_masters_version_detected, false);
        assert_eq!(result.older_workers_version_detected, false);
        let result = check_kubernetes_upgrade_status(
            "1.17",
            version_1_17.clone(),
            vec![version_1_16.clone()],
            event_details.clone(),
            &logger,
        )
        .unwrap();
        assert_eq!(result.required_upgrade_on.unwrap(), KubernetesNodesType::Workers); // then workers
        assert_eq!(result.deployed_masters_version, version_1_17);
        assert_eq!(result.deployed_workers_version, version_1_16);
        assert_eq!(result.older_masters_version_detected, false);
        assert_eq!(result.older_workers_version_detected, false);

        // everything is up to date, no upgrade required
        let result = check_kubernetes_upgrade_status(
            "1.17",
            version_1_17.clone(),
            vec![version_1_17.clone()],
            event_details.clone(),
            &logger,
        )
        .unwrap();
        assert!(result.required_upgrade_on.is_none());
        assert_eq!(result.older_masters_version_detected, false);
        assert_eq!(result.older_workers_version_detected, false);

        // downgrade should be detected
        let result = check_kubernetes_upgrade_status(
            "1.16",
            version_1_17.clone(),
            vec![version_1_17.clone()],
            event_details.clone(),
            &logger,
        )
        .unwrap();
        assert!(result.required_upgrade_on.is_none());
        assert_eq!(result.older_masters_version_detected, true);
        assert_eq!(result.older_workers_version_detected, true);

        // mixed workers version
        let result = check_kubernetes_upgrade_status(
            "1.17",
            version_1_17.clone(),
            vec![version_1_17.clone(), version_1_16.clone()],
            event_details,
            &logger,
        )
        .unwrap();
        assert_eq!(result.required_upgrade_on.unwrap(), KubernetesNodesType::Workers);
        assert_eq!(result.deployed_masters_version, version_1_17);
        assert_eq!(result.deployed_workers_version, version_1_16);
        assert_eq!(result.older_masters_version_detected, false); // not true because we're in an upgrade process
        assert_eq!(result.older_workers_version_detected, false); // not true because we're in an upgrade process
    }

    #[allow(dead_code)]
    pub fn print_kubernetes_version(provider_version: &VersionsNumber, provider: &VersionsNumber) {
        println!(
            "Provider version: {} | Wished version: {} | Is upgrade required: {:?}",
            provider_version.clone(),
            provider.clone(),
            compare_kubernetes_cluster_versions_for_upgrade(provider_version, provider)
                .unwrap()
                .message
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
                wished_version: VersionsNumber::new("1".to_string(), Some("16".to_string()), None, None),
            },
            KubernetesVersionToCheck {
                json: kubectl_version_do,
                wished_version: VersionsNumber::new("1".to_string(), Some("18".to_string()), None, None),
            },
        ];

        for mut provider in validate_providers {
            let provider_server_version: KubernetesVersion = serde_json::from_str(provider.json).unwrap();
            let provider_version = VersionsNumber::from_str(
                format!(
                    "{}",
                    VersionsNumber::new(
                        provider_server_version.server_version.major,
                        Some(provider_server_version.server_version.minor),
                        None,
                        None,
                    ),
                )
                .as_str(),
            )
            .expect("wrong kubernetes cluster version");

            // upgrade is not required
            //print_kubernetes_version(&provider_version, &provider.wished_version);
            assert_eq!(
                compare_kubernetes_cluster_versions_for_upgrade(&provider_version, &provider.wished_version)
                    .unwrap()
                    .upgraded_required,
                false
            );

            // upgrade is required
            let add_one_version = provider.wished_version.minor.unwrap().parse::<i32>().unwrap() + 1;
            provider.wished_version.minor = Some(add_one_version.to_string());
            //print_kubernetes_version(&provider_version, &provider.wished_version);
            assert!(
                compare_kubernetes_cluster_versions_for_upgrade(&provider_version, &provider.wished_version)
                    .unwrap()
                    .upgraded_required
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
                wished_version: VersionsNumber::new("1".to_string(), Some("16".to_string()), None, None),
            },
            KubernetesVersionToCheck {
                json: kubectl_version_do,
                wished_version: VersionsNumber::new("1".to_string(), Some("18".to_string()), None, None),
            },
        ];

        for mut provider in validate_providers {
            let provider_server_version: KubernetesList<KubernetesNode> =
                serde_json::from_str(provider.json).expect("Can't read workers json from {} provider");
            for node in provider_server_version.items {
                let kubelet = VersionsNumber::from_str(&node.status.node_info.kubelet_version).unwrap();
                let kube_proxy = VersionsNumber::from_str(&node.status.node_info.kube_proxy_version).unwrap();

                // upgrade is not required
                //print_kubernetes_version(&provider_version, &provider.wished_version);
                assert_eq!(
                    compare_kubernetes_cluster_versions_for_upgrade(&kubelet, &provider.wished_version)
                        .unwrap()
                        .upgraded_required,
                    false
                );
                assert_eq!(
                    compare_kubernetes_cluster_versions_for_upgrade(&kube_proxy, &provider.wished_version)
                        .unwrap()
                        .upgraded_required,
                    false
                );

                // upgrade is required
                let kubelet_add_one_version =
                    provider.wished_version.minor.clone().unwrap().parse::<i32>().unwrap() + 1;
                provider.wished_version.minor = Some(kubelet_add_one_version.to_string());
                //print_kubernetes_version(&provider_version, &provider.wished_version);
                assert!(
                    compare_kubernetes_cluster_versions_for_upgrade(&kubelet, &provider.wished_version)
                        .unwrap()
                        .upgraded_required
                );
            }
        }
    }

    #[test]
    pub fn test_k8s_milli_cpu_convert() {
        let milli_cpu = "250m".to_string();
        let int_cpu = "2".to_string();

        assert_eq!(convert_k8s_cpu_value_to_f32(milli_cpu).unwrap(), 0.25_f32);
        assert_eq!(convert_k8s_cpu_value_to_f32(int_cpu).unwrap(), 2_f32);
    }

    #[test]
    pub fn test_cpu_set() {
        let v = vec![];
        let listener_helper = ListenersHelper::new(&v);
        let logger = StdIoLogger::new();
        let execution_id = "execution_id";
        let context_id = "context_id";
        let organization_id = "organization_id";
        let cluster_id = "cluster_id";

        let event_details = EventDetails::new(
            Some(Aws),
            QoveryIdentifier::new_from_long_id(organization_id.to_string()),
            QoveryIdentifier::new_from_long_id(cluster_id.to_string()),
            QoveryIdentifier::new_from_long_id(execution_id.to_string()),
            Some("region_fake".to_string()),
            Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::Kubernetes(cluster_id.to_string(), format!("{}-name", cluster_id)),
        );

        let mut total_cpu = "0.25".to_string();
        let mut cpu_burst = "1".to_string();
        assert_eq!(
            validate_k8s_required_cpu_and_burstable(
                &listener_helper,
                execution_id,
                context_id,
                total_cpu,
                cpu_burst,
                event_details.clone(),
                &logger
            )
            .unwrap(),
            CpuLimits {
                cpu_request: "0.25".to_string(),
                cpu_limit: "1".to_string()
            }
        );

        total_cpu = "1".to_string();
        cpu_burst = "0.5".to_string();
        assert_eq!(
            validate_k8s_required_cpu_and_burstable(
                &listener_helper,
                execution_id,
                context_id,
                total_cpu,
                cpu_burst,
                event_details,
                &logger
            )
            .unwrap(),
            CpuLimits {
                cpu_request: "1".to_string(),
                cpu_limit: "1".to_string()
            }
        );
    }
}
