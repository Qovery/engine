use crate::environment::models::types::VersionsNumber;
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, EventMessage, InfrastructureStep};
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::gke::helm_charts::GkeHelmsDeployment;
use crate::infrastructure::action::gke::GkeQoveryTerraformOutput;
use crate::infrastructure::action::kubeconfig_helper::update_kubeconfig_file;
use crate::infrastructure::action::kubectl_utils::check_workers_on_create;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::gcp::Gke;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::object_storage::ObjectStorage;
use crate::services::kubernetes_api_deprecation_service::KubernetesApiDeprecationServiceGranuality;
use crate::utilities::envs_to_string;
use std::path::PathBuf;

pub(super) fn create_gke_cluster(
    cluster: &Gke,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));
    let kube_client = infra_ctx.mk_kube_client()?;

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

    if !infra_ctx.context().is_first_cluster_deployment() {
        let cloud_provider = infra_ctx.cloud_provider();
        let target_kubernetes_version = VersionsNumber::from(match cluster.version().next_version() {
            Some(v) => v,
            None => cluster.version().clone(),
        });
        logger.info(format!(
            "Check if cluster has calls to deprecated kubernetes API for version `{}`",
            target_kubernetes_version
        ));
        match infra_ctx
            .kubernetes_api_deprecation_service()
            .is_cluster_fully_compatible_with_kubernetes_version(
                cluster.kubeconfig_local_file_path().as_path(),
                Some(&target_kubernetes_version),
                &cloud_provider.credentials_environment_variables(),
                KubernetesApiDeprecationServiceGranuality::WithQoveryMetadata {
                    kube_client: kube_client.client(),
                },
            ) {
            Ok(_) => logger.info("Cluster has no calls to deprecated kubernetes API calls"),
            Err(e) => {
                // Non blocking error, just more FYI for user, to act on it if needed before upgrading
                let deprecation_error = EngineError::new_k8s_deprecated_api_calls_found(
                    event_details.clone(),
                    &target_kubernetes_version,
                    e,
                );
                logger.warn(EventMessage::from(deprecation_error));
            }
        }
    }

    Ok(())
}

fn create_object_storage(
    cluster: &Gke,
    logger: &impl InfraLogger,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    logger.info("Create Qovery managed object storage buckets.");
    for bucket_name in &[&cluster.logs_bucket_name()] {
        let existing_bucket = cluster
            .object_storage
            .create_bucket(bucket_name, cluster.advanced_settings.resource_ttl(), true)
            .map_err(|e| Box::new(EngineError::new_object_storage_error(event_details.clone(), e)))?;

        logger.info(format!("Object storage bucket {} already exists", &bucket_name));
        // Update set versioning to true if not activated on the bucket (bucket created before this option was enabled)
        // This can be removed at some point in the future, just here to handle legacy GCP buckets
        // TODO(ENG-1736): remove this update once all existing buckets have versioning activated
        if existing_bucket.versioning_activated {
            continue;
        }

        if let Err(err) = cluster.object_storage.update_bucket(bucket_name, true) {
            let error = EngineError::new_object_storage_error(event_details.clone(), err);
            return Err(Box::new(error));
        }
    }
    Ok(())
}
