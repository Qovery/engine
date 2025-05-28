use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, InfrastructureStep};
use crate::infrastructure::action::azure::AksQoveryTerraformOutput;
use crate::infrastructure::action::azure::helm_charts::AksHelmsDeployment;
use crate::infrastructure::action::cluster_outputs_helper::update_cluster_outputs;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::kubeconfig_helper::update_kubeconfig_file;
use crate::infrastructure::action::kubectl_utils::check_workers_on_create;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::container_registry::RegistryTags;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::azure::aks::AKS;
use crate::infrastructure::models::object_storage::azure_object_storage::StorageAccount;
use crate::utilities::envs_to_string;
use std::path::PathBuf;

pub(super) fn create_aks_cluster(
    cluster: &AKS,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));

    logger.info("Preparing AKS cluster deployment.");

    logger.info("Deploying AKS cluster.");

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
    let qovery_terraform_output: AksQoveryTerraformOutput = tf_resources.create(&logger)?;
    update_kubeconfig_file(cluster, &qovery_terraform_output.kubeconfig)?;
    if let Err(err) = update_cluster_outputs(cluster, &qovery_terraform_output) {
        logger.info(format!(
            "Failed to update outputs for cluster {}: {}",
            qovery_terraform_output.cluster_id, err
        ));
    }

    // Ensure all nodes are ready on Kubernetes
    check_workers_on_create(cluster, infra_ctx.cloud_provider(), None)
        .map_err(|e| Box::new(EngineError::new_k8s_node_not_ready(event_details.clone(), e)))?;

    logger.info("Kubernetes nodes have been successfully created");

    // Create Qovery managed blob container
    if let Err(err) = create_object_storage(
        cluster,
        &StorageAccount {
            access_key: qovery_terraform_output
                .main_storage_account_primary_access_key
                .to_string(),
            account_name: qovery_terraform_output.main_storage_account_name.to_string(),
        },
        &logger,
        event_details.clone(),
    ) {
        logger.error(*err.clone(), None::<&str>);
        return Err(err);
    }

    // Create cluster container registry
    if let Err(err) = create_container_registry(infra_ctx, event_details.clone()) {
        logger.error(*err.clone(), None::<&str>);
        return Err(err);
    }

    let helms_deployments = AksHelmsDeployment::new(
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

fn create_container_registry(
    infra_ctx: &InfrastructureContext,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    let azure_cr = infra_ctx
        .container_registry()
        .as_azure_container_registry()
        .map_err(|e| EngineError::new_container_registry_error(event_details.clone(), e))?;
    let kubernetes = infra_ctx.kubernetes();

    azure_cr
        .create_repository_in_resource_group(
            Some(infra_ctx.kubernetes().cluster_name().as_str()), // Create the registry in the same resource group as the cluster
            infra_ctx.kubernetes().cluster_name().as_str(),
            kubernetes.advanced_settings().registry_image_retention_time_sec,
            RegistryTags {
                cluster_id: Some(kubernetes.long_id().to_string()),
                environment_id: None,
                project_id: None,
                resource_ttl: kubernetes.advanced_settings().resource_ttl(),
            },
        )
        .map_err(|e| EngineError::new_container_registry_error(event_details.clone(), e))?;

    Ok(())
}

fn create_object_storage(
    cluster: &AKS,
    storage_account: &StorageAccount,
    logger: &impl InfraLogger,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    logger.info("Create Qovery managed blob container.");
    for bucket_name in &[&cluster.logs_bucket_name()] {
        match cluster.blob_storage.bucket_exists(storage_account, bucket_name) {
            true => {
                // bucket already exists, just update it, do nothing since update bucket is not yet available via SDK
                // TODO(benjaminch): use update bucket
                logger.info(format!("Blob container {} already exists", &bucket_name));
                // let existing_bucket = cluster
                //     .blob_storage
                //     .get_bucket(bucket_name)
                //     .map_err(|e| Box::new(EngineError::new_object_storage_error(event_details.clone(), e)))?
                // cluster
                //     .blob_storage
                //     .update_bucket(
                //         bucket_name,
                //         cluster.advanced_settings.resource_ttl(),
                //         true,
                //         cluster.advanced_settings.object_storage_enable_logging,
                //         existing_bucket.labels.clone(),
                //     )
                //     .map_err(|e| Box::new(EngineError::new_object_storage_error(event_details.clone(), e)))?;
            }
            false => {
                cluster
                    .blob_storage
                    .create_bucket(
                        storage_account,
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
