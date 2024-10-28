use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::regions::AwsZone;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups, NodeGroupsFormat};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::CloudProvider;
use crate::cmd::kubectl::kubectl_exec_api_custom_metrics;
use crate::cmd::terraform::{terraform_apply_with_tf_workers_resources, terraform_init_validate_state_list};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::dns_provider::DnsProvider;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventMessage, InfrastructureStep, Stage};
use crate::infrastructure_action::eks::karpenter::Karpenter;
use crate::infrastructure_action::eks::nodegroup::should_update_desired_nodes;
use crate::infrastructure_action::eks::tera_context::eks_tera_context;
use crate::infrastructure_action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure_action::eks::AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
use crate::io_models::context::Features;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use retry::delay::Fixed;
use retry::{Error, OperationResult};

pub fn pause_eks_cluster(
    infra_ctx: &InfrastructureContext,
    kubernetes: &dyn Kubernetes,
    cloud_provider: &dyn CloudProvider,
    dns_provider: &dyn DnsProvider,
    template_directory: &str,
    aws_zones: &[AwsZone],
    node_groups: &[NodeGroups],
    options: &Options,
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));

    kubernetes.logger().log(EngineEvent::Info(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
        EventMessage::new_from_safe("Preparing cluster pause.".to_string()),
    ));

    let temp_dir = kubernetes.temp_dir();

    let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), kubernetes, cloud_provider) {
        Ok(value) => Some(value),
        Err(_) => None,
    };

    let node_groups_with_desired_states = should_update_desired_nodes(
        event_details.clone(),
        kubernetes,
        KubernetesClusterAction::Pause,
        node_groups,
        aws_eks_client,
    )?;

    // in case error, this should not be a blocking error
    let mut cluster_upgrade_timeout_in_min = AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
    if let Ok(kube_client) = infra_ctx.mk_kube_client() {
        let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
            .unwrap_or(Vec::with_capacity(0));

        let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Pause);
        cluster_upgrade_timeout_in_min = timeout;

        if let Some(x) = message {
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
        }
    };

    // generate terraform files and copy them into temp dir
    let mut context = eks_tera_context(
        kubernetes,
        cloud_provider,
        dns_provider,
        aws_zones,
        &node_groups_with_desired_states,
        options,
        cluster_upgrade_timeout_in_min,
        false,
        advanced_settings,
        qovery_allowed_public_access_cidrs,
    )?;

    // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
    let worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
    context.insert("eks_worker_nodes", &worker_nodes);

    if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
        template_directory,
        temp_dir.to_string_lossy().as_ref(),
        context,
    ) {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap-{type}/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap-{type}/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        )));
    }

    // pause: only select terraform workers elements to pause to avoid applying on the whole config
    // this to avoid failures because of helm deployments on removing workers nodes
    let tf_workers_resources = match terraform_init_validate_state_list(
        temp_dir.to_string_lossy().as_ref(),
        cloud_provider.credentials_environment_variables().as_slice(),
        &TerraformValidators::Default,
    ) {
        Ok(x) => {
            let mut tf_workers_resources_name = Vec::new();
            for name in x.raw_std_output {
                if name.starts_with("aws_eks_node_group.") {
                    tf_workers_resources_name.push(name);
                }
            }
            tf_workers_resources_name
        }
        Err(e) => {
            let error = EngineError::new_terraform_error(event_details, e);
            kubernetes.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(Box::new(error));
        }
    };

    if tf_workers_resources.is_empty() && !kubernetes.is_karpenter_enabled() {
        kubernetes.logger().log(EngineEvent::Warning(
            event_details,
            EventMessage::new_from_safe(
                "Could not find workers resources in terraform state. Cluster seems already paused.".to_string(),
            ),
        ));
        return Ok(());
    }

    let kubernetes_config_file_path = kubernetes.kubeconfig_local_file_path();

    // pause: wait 1h for the engine to have 0 running jobs before pausing and avoid getting unreleased lock (from helm or terraform for example)
    if options.qovery_engine_location == EngineLocation::ClientSide {
        match kubernetes.context().is_feature_enabled(&Features::MetricsHistory) {
            true => {
                let metric_name = "taskmanager_nb_running_tasks";
                let wait_engine_job_finish = retry::retry(Fixed::from_millis(60000).take(60), || {
                    return match kubectl_exec_api_custom_metrics(
                        &kubernetes_config_file_path,
                        cloud_provider.credentials_environment_variables(),
                        "qovery",
                        None,
                        metric_name,
                    ) {
                        Ok(metrics) => {
                            let mut current_engine_jobs = 0;

                            for metric in metrics.items {
                                match metric.value.parse::<i32>() {
                                    Ok(job_count) if job_count > 0 => current_engine_jobs += 1,
                                    Err(e) => {
                                        return OperationResult::Retry(EngineError::new_cannot_get_k8s_api_custom_metrics(
                                            event_details.clone(),
                                            CommandError::new("Error while looking at the API metric value".to_string(), Some(e.to_string()), None)));
                                    }
                                    _ => {}
                                }
                            }

                            if current_engine_jobs == 0 {
                                OperationResult::Ok(())
                            } else {
                                OperationResult::Retry(EngineError::new_cannot_pause_cluster_tasks_are_running(event_details.clone(), None))
                            }
                        }
                        Err(e) => {
                            OperationResult::Retry(
                                EngineError::new_cannot_get_k8s_api_custom_metrics(event_details.clone(), e))
                        }
                    };
                });

                match wait_engine_job_finish {
                    Ok(_) => {
                        kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("No current running jobs on the Engine, infrastructure pause is allowed to start".to_string())));
                    }
                    Err(Error { error, .. }) => {
                        return Err(Box::new(error));
                    }
                }
            }
            false => kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Engines are running Client side, but metric history flag is disabled. You will encounter issues during cluster lifecycles if you do not enable metric history".to_string()))),
        }
    }

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Pausing cluster deployment.".to_string()),
    ));

    if kubernetes.is_karpenter_enabled() {
        let kube_client = infra_ctx.mk_kube_client()?;
        let kubernetes = kubernetes.as_eks().expect("expected EKS cluster here");
        block_on(Karpenter::pause(kubernetes, cloud_provider, &kube_client))?;
    }

    match terraform_apply_with_tf_workers_resources(
        temp_dir.to_string_lossy().as_ref(),
        tf_workers_resources,
        cloud_provider.credentials_environment_variables().as_slice(),
        &TerraformValidators::Default,
    ) {
        Ok(_) => {
            let message = format!("Kubernetes cluster {} successfully paused", kubernetes.name());
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details, EventMessage::new_from_safe(message)));

            Ok(())
        }
        Err(e) => Err(Box::new(EngineError::new_terraform_error(event_details, e))),
    }
}
