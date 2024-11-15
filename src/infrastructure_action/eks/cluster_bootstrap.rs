use crate::cloud_provider::aws::kubernetes::eks::EKS;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::{InfrastructureStep, Stage};
use crate::infrastructure_action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure_action::eks::tera_context::eks_tera_context;
use crate::infrastructure_action::eks::{AwsEksQoveryTerraformOutput, AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION};
use crate::infrastructure_action::InfraLogger;
use crate::utilities::envs_to_string;
use retry::delay::Fixed;
use retry::{Error, OperationResult};

pub fn bootstrap_eks_cluster(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    if !kubernetes.is_karpenter_enabled() {
        logger.info("No need to bootstrap EKS cluster because Karpenter is disabled");
        return Ok(());
    }

    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));

    logger.info(format!("Preparing {} cluster bootstrap.", kubernetes.kind()));
    let temp_dir = kubernetes.temp_dir();

    let cluster_upgrade_timeout_in_min = AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;

    // generate terraform files and copy them into temp dir
    let tera_context = eks_tera_context(
        kubernetes,
        infra_ctx.cloud_provider(),
        infra_ctx.dns_provider(),
        kubernetes.zones.as_slice(),
        &[],
        &kubernetes.options,
        cluster_upgrade_timeout_in_min,
        true,
        &kubernetes.advanced_settings,
        kubernetes.qovery_allowed_public_access_cidrs.as_ref(),
    )?;

    logger.info(format!("Bootstraping {} cluster.", kubernetes.kind()));
    let tf_action = TerraformInfraResources::new(
        tera_context.clone(),
        kubernetes.template_directory.join("terraform"),
        temp_dir.join("terraform_bootstrap"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        infra_ctx.context().is_dry_run_deploy(),
    );

    let tf_apply_result = retry::retry(Fixed::from_millis(3000).take(1), || {
        let qovery_terraform_output: Result<AwsEksQoveryTerraformOutput, Box<EngineError>> = tf_action.create(&logger);

        match qovery_terraform_output {
            Ok(output) => OperationResult::Ok(output),
            Err(e) => OperationResult::Retry(e),
        }
    });

    match tf_apply_result {
        Ok(_output) => Ok(()),
        Err(Error { error, .. }) => Err(error),
    }
}
