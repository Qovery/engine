use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::eksanywhere::helm_charts::EksAnywhereHelmsDeployment;
use crate::infrastructure::action::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhere;
use std::path::PathBuf;

pub(super) fn install_eks_anywhere_charts(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));

    logger.info("Deploying charts for Eks Anywhere cluster.");

    write_kubeconfig_on_disk(
        &cluster.kubeconfig_local_file_path(),
        &cluster.kubeconfig,
        cluster.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
    )?;

    let helms_deployments = EksAnywhereHelmsDeployment::new(
        HelmInfraContext::new(
            tera::Context::new(),
            PathBuf::from(infra_ctx.context().lib_root_dir()),
            cluster.template_directory.clone(),
            cluster.temp_dir().join("helms"),
            event_details.clone(),
            vec![],
            cluster.context().is_dry_run_deploy(),
        ),
        cluster,
    );
    helms_deployments.deploy_charts(infra_ctx, &logger)?;

    Ok(())
}
