use crate::cloud_provider::helm::deploy_charts_levels;
use crate::cloud_provider::kubeconfig_helper::put_kubeconfig_file_to_object_storage;
use crate::cloud_provider::kubectl_utils::check_workers_on_create;
use crate::cloud_provider::kubernetes::{is_kubernetes_upgrade_required, Kubernetes};
use crate::cloud_provider::scaleway::kubernetes::{Kapsule, ScwNodeGroupErrors};
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsScaleway};
use crate::cmd::kubectl_utils::kubectl_are_qovery_infra_pods_executed;
use crate::cmd::terraform::{terraform_init_validate_plan_apply, terraform_output};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventMessage, InfrastructureStep};
use crate::infrastructure_action::scaleway::helm_charts::{kapsule_helm_charts, KapsuleChartsConfigPrerequisites};
use crate::infrastructure_action::scaleway::nodegroup::{get_existing_sanitized_node_groups, get_node_group_info};
use crate::infrastructure_action::scaleway::tera_context::kapsule_tera_context;
use crate::infrastructure_action::scaleway::ScalewayQoveryTerraformOutput;
use crate::infrastructure_action::InfrastructureAction;
use crate::io_models::context::Features;
use crate::models::domain::ToHelmString;
use crate::models::third_parties::LetsEncryptConfig;
use crate::object_storage::ObjectStorage;
use crate::string::terraform_list_format;
use itertools::Itertools;
use retry::delay::Fixed;
use retry::OperationResult;

pub fn create_kapsule_cluster(cluster: &Kapsule, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));

    // TODO(DEV-1061): remove legacy logger
    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing SCW cluster deployment.".to_string()),
    ));

    // upgrade cluster instead if required
    if !cluster.context().is_first_cluster_deployment() {
        match is_kubernetes_upgrade_required(
            cluster.kubeconfig_local_file_path(),
            cluster.version().clone(),
            infra_ctx.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            cluster.logger(),
            None,
        ) {
            Ok(x) => {
                if x.required_upgrade_on.is_some() {
                    cluster.upgrade_cluster(infra_ctx, x)?;
                } else {
                    cluster.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                    ))
                }
            }
            Err(e) => {
                // Log a warning, this error is not blocking
                cluster.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(
                        "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                        Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }
        };
    }

    let temp_dir = cluster.temp_dir();

    // generate terraform files and copy them into temp dir
    let context = kapsule_tera_context(cluster, infra_ctx)?;

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

    let dirs_to_be_copied_to = vec![
        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        (
            format!("{}/common/bootstrap/charts", cluster.context().lib_root_dir()),
            format!("{}/common/charts", temp_dir.to_string_lossy()),
        ),
        // copy lib/common/bootstrap/chart_values directory (and sub directory) into the lib/scaleway/bootstrap/common/chart_values directory.
        (
            format!("{}/common/bootstrap/chart_values", cluster.context().lib_root_dir()),
            format!("{}/common/chart_values", temp_dir.to_string_lossy()),
        ),
    ];
    for (source_dir, target_dir) in dirs_to_be_copied_to {
        if let Err(e) = crate::template::copy_non_template_files(&source_dir, target_dir.as_str()) {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                source_dir,
                target_dir,
                e,
            )));
        }
    }

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Deploying SCW cluster.".to_string()),
    ));

    // TODO(benjaminch): move this elsewhere
    // Create object-storage buckets
    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Create Qovery managed object storage buckets".to_string()),
    ));
    if let Err(e) = cluster.object_storage.create_bucket(
        cluster.kubeconfig_bucket_name().as_str(),
        cluster.advanced_settings().resource_ttl(),
        false,
    ) {
        let error = EngineError::new_object_storage_error(event_details, e);
        cluster.logger().log(EngineEvent::Error(error.clone(), None));
        return Err(Box::new(error));
    }

    // Logs bucket
    if let Err(e) = cluster.object_storage.create_bucket(
        cluster.logs_bucket_name().as_str(),
        cluster.advanced_settings().resource_ttl(),
        false,
    ) {
        let error = EngineError::new_object_storage_error(event_details, e);
        cluster.logger().log(EngineEvent::Error(error.clone(), None));
        return Err(Box::new(error));
    }

    // terraform deployment dedicated to cloud resources
    if let Err(e) = terraform_init_validate_plan_apply(
        temp_dir.to_string_lossy().as_ref(),
        cluster.context().is_dry_run_deploy(),
        &[],
        &TerraformValidators::Default,
    ) {
        return Err(Box::new(EngineError::new_terraform_error(event_details, e)));
    }
    let qovery_terraform_output: ScalewayQoveryTerraformOutput = terraform_output(
        temp_dir.to_string_lossy().as_ref(),
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    )
    .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

    // push config file to object storage
    let kubeconfig_path = cluster.kubeconfig_local_file_path();
    put_kubeconfig_file_to_object_storage(cluster, &cluster.object_storage)?;

    let cluster_info = cluster.get_scw_cluster_info()?;
    if cluster_info.is_none() {
        return Err(Box::new(EngineError::new_no_cluster_found_error(
            event_details,
            CommandError::new_from_safe_message("Error, no cluster found from the Scaleway API".to_string()),
        )));
    }

    let cluster_endpoint = match cluster_info.clone() {
        Some(x) => x.cluster_url,
        None => None,
    };
    let cluster_secrets = ClusterSecrets::new_scaleway(ClusterSecretsScaleway::new(
        infra_ctx.cloud_provider().access_key_id(),
        infra_ctx.cloud_provider().secret_access_key(),
        cluster.options.scaleway_project_id.to_string(),
        cluster.region().to_string(),
        cluster.default_zone().unwrap_or("").to_string(),
        None,
        cluster_endpoint,
        cluster.kind(),
        infra_ctx.cloud_provider().name().to_string(),
        cluster.long_id().to_string(),
        cluster.options.grafana_admin_user.clone(),
        cluster.options.grafana_admin_password.clone(),
        infra_ctx.cloud_provider().organization_long_id().to_string(),
        cluster.context().is_test_cluster(),
    ));

    // send cluster info with kubeconfig
    // create vault connection (Vault connectivity should not be on the critical deployment path,
    // if it temporarily fails, just ignore it, data will be pushed on the next sync)
    let _ = cluster.update_vault_config(event_details.clone(), cluster_secrets, Some(&kubeconfig_path));

    let current_nodegroups = match get_existing_sanitized_node_groups(
        cluster,
        cluster_info.expect("A cluster should be present at this create stage"),
    ) {
        Ok(x) => x,
        Err(e) => {
            match e {
                ScwNodeGroupErrors::CloudProviderApiError(c) => {
                    return Err(Box::new(EngineError::new_missing_api_info_from_cloud_provider_error(
                        event_details,
                        Some(c),
                    )));
                }
                ScwNodeGroupErrors::ClusterDoesNotExists(_) => cluster.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new_from_safe(
                        "Cluster do not exists, no node groups can be retrieved for upgrade check.".to_string(),
                    ),
                )),
                ScwNodeGroupErrors::MultipleClusterFound => {
                    return Err(Box::new(EngineError::new_multiple_cluster_found_expected_one_error(
                        event_details,
                        CommandError::new_from_safe_message(
                            "Error, multiple clusters found, can't match the correct node groups.".to_string(),
                        ),
                    )));
                }
                ScwNodeGroupErrors::NoNodePoolFound(_) => cluster.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new_from_safe(
                        "Cluster exists, but no node groups found for upgrade check.".to_string(),
                    ),
                )),
                ScwNodeGroupErrors::MissingNodePoolInfo => {
                    return Err(Box::new(EngineError::new_missing_api_info_from_cloud_provider_error(
                        event_details,
                        Some(CommandError::new_from_safe_message(
                            "Error with Scaleway API while trying to retrieve node pool info".to_string(),
                        )),
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
    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Ensuring all groups nodes are in ready state from the Scaleway API".to_string()),
    ));

    for ng in current_nodegroups {
        let res = retry::retry(
            // retry 10 min max per nodegroup until they are ready
            Fixed::from_millis(15000).take(80),
            || {
                cluster.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!(
                        "checking node group {}/{:?}, current status: {:?}",
                        &ng.name,
                        &ng.id.as_ref().unwrap_or(&"unknown".to_string()),
                        &ng.status
                    )),
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
                                cluster.logger().log(EngineEvent::Error(current_error.clone(), None));
                                OperationResult::Retry(current_error)
                            }
                            ScwNodeGroupErrors::ClusterDoesNotExists(c) => {
                                let current_error = EngineError::new_no_cluster_found_error(event_details.clone(), c);
                                cluster.logger().log(EngineEvent::Error(current_error.clone(), None));
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
                            ScwNodeGroupErrors::MissingNodePoolInfo => {
                                OperationResult::Retry(EngineError::new_missing_api_info_from_cloud_provider_error(
                                    event_details.clone(),
                                    None,
                                ))
                            }
                            ScwNodeGroupErrors::NodeGroupValidationError(c) => {
                                let current_error = EngineError::new_missing_api_info_from_cloud_provider_error(
                                    event_details.clone(),
                                    Some(c),
                                );
                                cluster.logger().log(EngineEvent::Error(current_error.clone(), None));
                                OperationResult::Retry(current_error)
                            }
                        };
                    }
                };
                match scw_ng.status == scaleway_api_rs::models::scaleway_k8s_v1_pool::Status::Ready {
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
    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("All node groups for this cluster are ready from cloud provider API".to_string()),
    ));

    // ensure all nodes are ready on Kubernetes
    match check_workers_on_create(cluster, infra_ctx.cloud_provider(), None) {
        Ok(_) => cluster.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Kubernetes nodes have been successfully created".to_string()),
        )),
        Err(e) => {
            return Err(Box::new(EngineError::new_k8s_node_not_ready(event_details, e)));
        }
    };

    // kubernetes helm deployments on the cluster
    let credentials_environment_variables: Vec<(String, String)> = infra_ctx
        .cloud_provider()
        .credentials_environment_variables()
        .into_iter()
        .map(|x| (x.0.to_string(), x.1.to_string()))
        .collect();

    if let Err(e) = kubectl_are_qovery_infra_pods_executed(&kubeconfig_path, &credentials_environment_variables) {
        cluster.logger().log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new("Didn't manage to restart all paused pods".to_string(), Some(e.to_string())),
        ));
    }

    let charts_prerequisites = KapsuleChartsConfigPrerequisites::new(
        infra_ctx.cloud_provider().organization_id().to_string(),
        infra_ctx.cloud_provider().organization_long_id(),
        cluster.short_id().to_string(),
        cluster.long_id,
        cluster.zone,
        cluster.options.qovery_engine_location.clone(),
        cluster.context().is_feature_enabled(&Features::LogsHistory),
        cluster.context().is_feature_enabled(&Features::MetricsHistory),
        cluster.context().is_feature_enabled(&Features::Grafana),
        infra_ctx.dns_provider().domain().to_helm_format_string(),
        terraform_list_format(
            infra_ctx
                .dns_provider()
                .resolvers()
                .iter()
                .map(|x| x.to_string())
                .collect(),
        ),
        infra_ctx.dns_provider().domain().root_domain().to_helm_format_string(),
        LetsEncryptConfig::new(
            cluster.options.tls_email_report.to_string(),
            cluster.context().is_test_cluster(),
        ),
        infra_ctx.dns_provider().provider_configuration(),
        cluster.options.clone(),
        cluster.advanced_settings().clone(),
        qovery_terraform_output.loki_storage_config_scaleway_s3,
    );

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
    ));
    let helm_charts_to_deploy = kapsule_helm_charts(
        &charts_prerequisites,
        Some(temp_dir.to_string_lossy().as_ref()),
        &kubeconfig_path,
        &*cluster.context().qovery_api,
        cluster.customer_helm_charts_override(),
        infra_ctx.dns_provider().domain(),
    )
    .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?;

    deploy_charts_levels(
        infra_ctx.mk_kube_client()?.client(),
        &kubeconfig_path,
        credentials_environment_variables
            .iter()
            .map(|(l, r)| (l.as_str(), r.as_str()))
            .collect_vec()
            .as_slice(),
        helm_charts_to_deploy,
        cluster.context().is_dry_run_deploy(),
        Some(&infra_ctx.kubernetes().helm_charts_diffs_directory()),
    )
    .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))
}
