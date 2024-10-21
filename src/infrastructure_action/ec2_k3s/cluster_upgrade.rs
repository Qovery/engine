use crate::cloud_provider::aws::kubernetes::ec2::EC2;
use crate::cloud_provider::kubernetes::{Kubernetes, KubernetesUpgradeStatus};
use crate::cloud_provider::models::NodeGroupsWithDesiredState;
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsAws};
use crate::cmd::terraform::{terraform_init_validate_plan_apply, terraform_output};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EventMessage, InfrastructureStep, Stage};
use crate::infrastructure_action::eks::eks_tera_context;
use crate::infrastructure_action::AwsEc2QoveryTerraformOutput;
use chrono::Duration;

pub fn ec2_k3s_cluster_upgrade(
    kubernetes: &EC2,
    infra_ctx: &InfrastructureContext,
    _kubernetes_upgrade_status: KubernetesUpgradeStatus,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Start preparing EC2 node upgrade process".to_string()),
    ));

    let temp_dir = kubernetes.temp_dir();

    // generate terraform files and copy them into temp dir
    let context = eks_tera_context(
        kubernetes,
        infra_ctx.cloud_provider(),
        infra_ctx.dns_provider(),
        &kubernetes.zones,
        &[NodeGroupsWithDesiredState::new_from_node_groups(
            &kubernetes.node_group_from_instance_type(),
            1,
            false,
        )],
        &kubernetes.options,
        Duration::minutes(0), // not used for EC2
        false,
        &kubernetes.advanced_settings,
        None,
    )?;

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(kubernetes.template_directory.as_str(), temp_dir, context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            kubernetes.template_directory.to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
    let common_bootstrap_charts = format!("{}/common/bootstrap/charts", kubernetes.context.lib_root_dir());
    if let Err(e) =
        crate::template::copy_non_template_files(common_bootstrap_charts.as_str(), common_charts_temp_dir.as_str())
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            common_bootstrap_charts,
            common_charts_temp_dir,
            e,
        )));
    }

    terraform_init_validate_plan_apply(
        temp_dir.to_string_lossy().as_ref(),
        kubernetes.context.is_dry_run_deploy(),
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
        &TerraformValidators::Default,
    )
    .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

    // update Vault with new cluster information
    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Ensuring the upgrade has successfully been performed...".to_string()),
    ));

    let qovery_terraform_output: AwsEc2QoveryTerraformOutput = terraform_output(
        temp_dir.to_string_lossy().as_ref(),
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    )
    .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

    let cluster_secrets = ClusterSecrets::new_aws_eks(ClusterSecretsAws::new(
        infra_ctx.cloud_provider().access_key_id(),
        kubernetes.region().to_string(),
        infra_ctx.cloud_provider().secret_access_key(),
        None,
        Some(qovery_terraform_output.aws_ec2_public_hostname.clone()),
        kubernetes.kind(),
        kubernetes.cluster_name(),
        kubernetes.long_id().to_string(),
        kubernetes.options.grafana_admin_user.clone(),
        kubernetes.options.grafana_admin_password.clone(),
        infra_ctx.cloud_provider().organization_long_id().to_string(),
        kubernetes.context().is_test_cluster(),
    ));

    if let Err(e) = kubernetes.update_vault_config(
        event_details.clone(),
        cluster_secrets,
        Some(&kubernetes.kubeconfig_local_file_path()),
    ) {
        kubernetes.logger().log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new(
                "Wasn't able to update Vault information for this EC2 instance".to_string(),
                Some(e.to_string()),
            ),
        ));
    };

    kubernetes.logger().log(EngineEvent::Info(
        event_details,
        EventMessage::new_from_safe("Kubernetes nodes have been successfully upgraded".to_string()),
    ));

    Ok(())
}
