use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::kubectl_utils::check_workers_on_pause;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::Kapsule;
use crate::io_models::models::NodeGroupsFormat;
use crate::utilities::envs_to_string;

pub fn pause_kapsule_cluster(
    cluster: &Kapsule,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Pause));
    logger.info("Preparing cluster pause.");

    let temp_dir = cluster.temp_dir();

    // generate terraform files and copy them into temp dir
    let mut tera_context = cluster.to_infra_tera_context(infra_ctx)?;

    // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
    let scw_ks_worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
    tera_context.insert("scw_ks_worker_nodes", &scw_ks_worker_nodes);
    let tf_resources = TerraformInfraResources::new(
        tera_context,
        cluster.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        cluster.context().is_dry_run_deploy(),
    );

    logger.info("Pausing cluster deployment.");
    tf_resources.pause(&["scw_ks_worker_nodes"])?;

    if let Err(e) = check_workers_on_pause(cluster, infra_ctx.cloud_provider(), None) {
        return Err(Box::new(EngineError::new_k8s_node_not_ready(event_details, e)));
    };

    let message = format!("Kubernetes cluster {} successfully paused", cluster.name());
    logger.info(message);
    Ok(())
}
