use crate::cloud_provider::kubectl_utils::check_workers_on_pause;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::models::NodeGroupsFormat;
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::scaleway::kubernetes::Kapsule;
use crate::cmd::kubectl::kubectl_exec_api_custom_metrics;
use crate::cmd::terraform::{terraform_apply_with_tf_workers_resources, terraform_init_validate_state_list};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventMessage, InfrastructureStep};
use crate::infrastructure_action::scaleway::tera_context::kapsule_tera_context;
use crate::io_models::context::Features;
use retry::delay::Fixed;
use retry::OperationResult;

pub fn pause_kapsule_cluster(cluster: &Kapsule, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Pause));
    cluster.logger().log(EngineEvent::Info(
        cluster.get_event_details(Infrastructure(InfrastructureStep::Pause)),
        EventMessage::new_from_safe("Preparing cluster pause.".to_string()),
    ));

    let temp_dir = cluster.temp_dir();

    // generate terraform files and copy them into temp dir
    let mut context = kapsule_tera_context(cluster, infra_ctx)?;

    // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
    let scw_ks_worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
    context.insert("scw_ks_worker_nodes", &scw_ks_worker_nodes);

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(cluster.template_directory.as_str(), temp_dir, context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            cluster.template_directory.to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", cluster.context().lib_root_dir());
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
        &[],
        &TerraformValidators::Default,
    ) {
        Ok(x) => {
            let mut tf_workers_resources_name = Vec::new();
            for name in x.raw_std_output {
                if name.starts_with("scaleway_k8s_pool.") {
                    tf_workers_resources_name.push(name);
                }
            }
            tf_workers_resources_name
        }
        Err(e) => {
            let error = EngineError::new_terraform_error(event_details, e);
            cluster.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(Box::new(error));
        }
    };

    if tf_workers_resources.is_empty() {
        cluster.logger().log(EngineEvent::Warning(
            event_details,
            EventMessage::new_from_safe(
                "Could not find workers resources in terraform state. Cluster seems already paused.".to_string(),
            ),
        ));
        return Ok(());
    }

    let kubernetes_config_file_path = cluster.kubeconfig_local_file_path();

    // pause: wait 1h for the engine to have 0 running jobs before pausing and avoid getting unreleased lock (from helm or terraform for example)
    if cluster.options.qovery_engine_location == EngineLocation::ClientSide {
        match cluster.context().is_feature_enabled(&Features::MetricsHistory) {
            true => {
                let metric_name = "taskmanager_nb_running_tasks";
                let wait_engine_job_finish = retry::retry(Fixed::from_millis(60000).take(60), || {
                    return match kubectl_exec_api_custom_metrics(
                        &kubernetes_config_file_path,
                        infra_ctx.cloud_provider().credentials_environment_variables(),
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
                                        return OperationResult::Retry(EngineError::new_cannot_get_k8s_api_custom_metrics(event_details.clone(), CommandError::new("Error while looking at the API metric value".to_string(), Some(e.to_string()), None)));
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
                        cluster.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("No current running jobs on the Engine, infrastructure pause is allowed to start".to_string())));
                    }
                    Err(retry::Error { error, .. }) => {
                        return Err(Box::new(error));
                    }
                }
            }
            false => cluster.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Engines are running Client side, but metric history flag is disabled. You will encounter issues during cluster lifecycles if you do not enable metric history".to_string()))),
        }
    }

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Pausing cluster deployment.".to_string()),
    ));

    if let Err(e) = terraform_apply_with_tf_workers_resources(
        temp_dir.to_string_lossy().as_ref(),
        tf_workers_resources,
        &[],
        &TerraformValidators::Default,
    ) {
        return Err(Box::new(EngineError::new_terraform_error(event_details, e)));
    }

    if let Err(e) = check_workers_on_pause(cluster, infra_ctx.cloud_provider(), None) {
        return Err(Box::new(EngineError::new_k8s_node_not_ready(event_details, e)));
    };

    let message = format!("Kubernetes cluster {} successfully paused", cluster.name());
    cluster
        .logger()
        .log(EngineEvent::Info(event_details, EventMessage::new_from_safe(message)));
    Ok(())
}
