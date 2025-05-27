use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, InfrastructureStep};
use crate::infrastructure::action::cluster_outputs_helper::update_cluster_outputs;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::gke::GkeQoveryTerraformOutput;
use crate::infrastructure::action::gke::helm_charts::GkeHelmsDeployment;
use crate::infrastructure::action::kubeconfig_helper::update_kubeconfig_file;
use crate::infrastructure::action::kubectl_utils::check_workers_on_create;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::gcp::Gke;
use crate::infrastructure::models::object_storage::ObjectStorage;
use crate::utilities::envs_to_string;
use std::path::PathBuf;

pub(super) fn create_gke_cluster(
    cluster: &Gke,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));

    logger.info("Preparing GKE cluster deployment.");

    logger.info("Deploying GKE cluster.");
    if let Err(err) = create_object_storage(cluster, &logger, event_details.clone()) {
        logger.error(*err.clone(), None::<&str>);
        return Err(err);
    }

    // Terraform deployment dedicated to cloud resources
    let tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    let tf_resources = TerraformInfraResources::new(
        tera_context.clone(),
        cluster.template_directory.join("terraform"),
        cluster.temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        cluster.context().is_dry_run_deploy(),
    );
    let qovery_terraform_output: GkeQoveryTerraformOutput = tf_resources.create(&logger)?;
    update_kubeconfig_file(cluster, &qovery_terraform_output.kubeconfig)?;
    if let Err(err) = update_cluster_outputs(cluster, &qovery_terraform_output) {
        logger.info(format!(
            "Failed to update outputs for cluster {}: {}",
            qovery_terraform_output.cluster_id, err
        ));
    }

    // Configure kubectl to be able to connect to cluster
    let _ = cluster.configure_gcloud_for_cluster(infra_ctx); // TODO(ENG-1802): properly handle this error

    // Ensure all nodes are ready on Kubernetes
    check_workers_on_create(cluster, infra_ctx.cloud_provider(), None)
        .map_err(|e| Box::new(EngineError::new_k8s_node_not_ready(event_details.clone(), e)))?;
    logger.info("Kubernetes nodes have been successfully created");

    let helms_deployments = GkeHelmsDeployment::new(
        HelmInfraContext::new(
            tera_context,
            PathBuf::from(infra_ctx.context().lib_root_dir()),
            cluster.template_directory.clone(),
            cluster.temp_dir().join("helms"),
            event_details.clone(),
            vec![],
            cluster.context().is_dry_run_deploy(),
        ),
        qovery_terraform_output,
        cluster,
    );
    helms_deployments.deploy_charts(infra_ctx, &logger)?;

    Ok(())
}

fn create_object_storage(
    cluster: &Gke,
    logger: &impl InfraLogger,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    logger.info("Create Qovery managed object storage buckets.");

    for bucket_name in &[&cluster.logs_bucket_name(), &cluster.prometheus_bucket_name()] {
        match cluster.object_storage.bucket_exists(bucket_name) {
            true => {
                // bucket already exists, just update it
                logger.info(format!("Object storage bucket {} already exists", &bucket_name));
                let existing_bucket = cluster
                    .object_storage
                    .get_bucket(bucket_name)
                    .map_err(|e| Box::new(EngineError::new_object_storage_error(event_details.clone(), e)))?;
                cluster
                    .object_storage
                    .update_bucket(
                        bucket_name,
                        cluster.advanced_settings.resource_ttl(),
                        true,
                        cluster.advanced_settings.object_storage_enable_logging,
                        existing_bucket.labels.clone(),
                    )
                    .map_err(|e| Box::new(EngineError::new_object_storage_error(event_details.clone(), e)))?;
            }
            false => {
                cluster
                    .object_storage
                    .create_bucket(
                        bucket_name,
                        cluster.advanced_settings.resource_ttl(),
                        true,
                        cluster.advanced_settings.object_storage_enable_logging,
                    )
                    .map_err(|e| Box::new(EngineError::new_object_storage_error(event_details.clone(), e)))?;
            }
        }
    }
    Ok(())
}
