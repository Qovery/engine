use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, InfrastructureStep};
use crate::infrastructure::action::cluster_outputs_helper::update_cluster_outputs;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::kubectl_utils::check_workers_on_create;
use crate::infrastructure::action::scaleway::ScalewayQoveryTerraformOutput;
use crate::infrastructure::action::scaleway::helm_charts::KapsuleHelmsDeployment;
use crate::infrastructure::action::scaleway::nodegroup::{get_existing_sanitized_node_groups, get_node_group_info};
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::{Kapsule, ScwNodeGroupErrors};
use crate::infrastructure::models::object_storage::ObjectStorage;
use crate::utilities::envs_to_string;
use retry::OperationResult;
use retry::delay::Fixed;
use scaleway_api_rs::models::ScalewayPeriodK8sPeriodV1PeriodCluster;
use std::path::PathBuf;

pub fn create_kapsule_cluster(
    cluster: &Kapsule,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));
    logger.info("Preparing SCW cluster deployment.");

    let temp_dir = cluster.temp_dir();
    logger.info("Create Qovery managed object storage buckets");

    // Logs bucket
    if let Err(e) = cluster.object_storage.create_bucket(
        cluster.logs_bucket_name().as_str(),
        cluster.advanced_settings().resource_ttl(),
        false,
        cluster.advanced_settings().object_storage_enable_logging,
    ) {
        let error = EngineError::new_object_storage_error(event_details, e);
        logger.error(error.clone(), None::<&str>);
        return Err(Box::new(error));
    }

    // Prometheus bucket
    if let Err(e) = cluster.object_storage.create_bucket(
        cluster.prometheus_bucket_name().as_str(),
        cluster.advanced_settings().resource_ttl(),
        false,
        cluster.advanced_settings().object_storage_enable_logging,
    ) {
        let error = EngineError::new_object_storage_error(event_details, e);
        logger.error(error.clone(), None::<&str>);
        return Err(Box::new(error));
    }

    // terraform deployment dedicated to cloud resources
    let tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    let tf_action = TerraformInfraResources::new(
        tera_context.clone(),
        cluster.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        cluster.context().is_dry_run_deploy(),
    );
    let qovery_terraform_output: ScalewayQoveryTerraformOutput = tf_action.create(&logger)?;
    update_cluster_outputs(cluster, &qovery_terraform_output)?;

    let cluster_info = cluster.get_scw_cluster_info()?.ok_or_else(|| {
        Box::new(EngineError::new_no_cluster_found_error(
            event_details.clone(),
            CommandError::new_from_safe_message("Error, no cluster found from the Scaleway API".to_string()),
        ))
    })?;

    sanitize_node_groups(cluster, event_details.clone(), cluster_info, &logger)?;

    // ensure all nodes are ready on Kubernetes
    check_workers_on_create(cluster, infra_ctx.cloud_provider(), None)
        .map_err(|e| Box::new(EngineError::new_k8s_node_not_ready(event_details.clone(), e)))?;
    logger.info("Kubernetes nodes have been successfully created");

    // kubernetes helm deployments on the cluster
    let helms_deployments = KapsuleHelmsDeployment::new(
        HelmInfraContext::new(
            tera_context,
            PathBuf::from(infra_ctx.context().lib_root_dir()),
            cluster.template_directory.clone(),
            cluster.temp_dir().join("helms"),
            event_details.clone(),
            envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
            cluster.context().is_dry_run_deploy(),
        ),
        qovery_terraform_output,
        cluster,
    );
    helms_deployments.deploy_charts(infra_ctx, &logger)?;

    Ok(())
}

fn sanitize_node_groups(
    cluster: &Kapsule,
    event_details: EventDetails,
    cluster_info: ScalewayPeriodK8sPeriodV1PeriodCluster,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    if cluster.context().is_dry_run_deploy() {
        logger.info("👻 Dry run mode enabled, skipping node groups sanitization");
        return Ok(());
    }

    let current_nodegroups = match get_existing_sanitized_node_groups(cluster, cluster_info) {
        Ok(x) => x,
        Err(e) => {
            match e {
                ScwNodeGroupErrors::CloudProviderApiError(c) => {
                    return Err(Box::new(EngineError::new_missing_api_info_from_cloud_provider_error(
                        event_details,
                        Some(c),
                    )));
                }
                ScwNodeGroupErrors::ClusterDoesNotExists(_) => {
                    logger.warn("Cluster do not exists, no node groups can be retrieved for upgrade check.")
                }
                ScwNodeGroupErrors::MultipleClusterFound => {
                    return Err(Box::new(EngineError::new_multiple_cluster_found_expected_one_error(
                        event_details,
                        CommandError::new_from_safe_message(
                            "Error, multiple clusters found, can't match the correct node groups.".to_string(),
                        ),
                    )));
                }
                ScwNodeGroupErrors::NoNodePoolFound(_) => {
                    logger.warn("Cluster exists, but no node groups found for upgrade check.")
                }
                ScwNodeGroupErrors::MissingNodePoolInfo(name) => {
                    return Err(Box::new(EngineError::new_missing_api_info_from_cloud_provider_error(
                        event_details,
                        Some(CommandError::new_from_safe_message(format!(
                            "Error with Scaleway API while trying to retrieve node pool info. Missing {name} info"
                        ))),
                    )));
                }
                ScwNodeGroupErrors::NodeGroupValidationError(c) => {
                    return Err(Box::new(EngineError::new_missing_api_info_from_cloud_provider_error(
                        event_details,
                        Some(c),
                    )));
                }
            };
            Vec::with_capacity(0)
        }
    };

    // ensure all node groups are in ready state Scaleway side
    logger.info("Ensuring all groups nodes are in ready state from the Scaleway API");
    for ng in current_nodegroups {
        let res = retry::retry(
            // retry 10 min max per nodegroup until they are ready
            Fixed::from_millis(15000).take(80),
            || {
                logger.info(format!(
                    "checking node group {}/{:?}, current status: {:?}",
                    &ng.name,
                    &ng.id.as_ref().unwrap_or(&"unknown".to_string()),
                    &ng.status
                ));
                let pool_id = match &ng.id {
                    None => {
                        let msg = "node group id was expected to get info, but not found from Scaleway API".to_string();
                        return OperationResult::Retry(EngineError::new_missing_api_info_from_cloud_provider_error(
                            event_details.clone(),
                            Some(CommandError::new_from_safe_message(msg)),
                        ));
                    }
                    Some(x) => x,
                };
                let scw_ng = match get_node_group_info(cluster, pool_id.as_str()) {
                    Ok(x) => x,
                    Err(e) => {
                        return match e {
                            ScwNodeGroupErrors::CloudProviderApiError(c) => {
                                let current_error = EngineError::new_missing_api_info_from_cloud_provider_error(
                                    event_details.clone(),
                                    Some(c),
                                );
                                logger.warn(current_error.message(ErrorMessageVerbosity::SafeOnly));
                                OperationResult::Retry(current_error)
                            }
                            ScwNodeGroupErrors::ClusterDoesNotExists(c) => {
                                let current_error = EngineError::new_no_cluster_found_error(event_details.clone(), c);
                                logger.warn(current_error.message(ErrorMessageVerbosity::SafeOnly));
                                OperationResult::Retry(current_error)
                            }
                            ScwNodeGroupErrors::MultipleClusterFound => {
                                OperationResult::Retry(EngineError::new_multiple_cluster_found_expected_one_error(
                                    event_details.clone(),
                                    CommandError::new_from_safe_message(
                                        "Multiple cluster found while one was expected".to_string(),
                                    ),
                                ))
                            }
                            ScwNodeGroupErrors::NoNodePoolFound(_) => OperationResult::Ok(()),
                            ScwNodeGroupErrors::MissingNodePoolInfo(name) => {
                                OperationResult::Retry(EngineError::new_missing_api_info_from_cloud_provider_error(
                                    event_details.clone(),
                                    Some(CommandError::new_from_safe_message(format!(
                                        "Error with Scaleway API while trying to retrieve node pool info. Missing {name} info"
                                    ))),
                                ))
                            }
                            ScwNodeGroupErrors::NodeGroupValidationError(c) => {
                                let current_error = EngineError::new_missing_api_info_from_cloud_provider_error(
                                    event_details.clone(),
                                    Some(c),
                                );
                                logger.warn(current_error.message(ErrorMessageVerbosity::SafeOnly));
                                OperationResult::Retry(current_error)
                            }
                        };
                    }
                };
                match scw_ng.status == scaleway_api_rs::models::scaleway_period_k8s_period_v1_period_pool::Status::Ready
                {
                    true => OperationResult::Ok(()),
                    false => OperationResult::Retry(EngineError::new_k8s_node_not_ready(
                        event_details.clone(),
                        CommandError::new_from_safe_message(format!(
                            "waiting for node group {} to be ready, current status: {:?}",
                            &scw_ng.name, scw_ng.status
                        )),
                    )),
                }
            },
        );
        match res {
            Ok(_) => {}
            Err(retry::Error { error, .. }) => return Err(Box::new(error)),
        }
    }
    logger.info("All node groups for this cluster are ready from cloud provider API");

    Ok(())
}
