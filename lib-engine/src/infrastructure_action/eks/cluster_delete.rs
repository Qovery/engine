use super::helm_charts::karpenter::KarpenterChart;
use super::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use super::helm_charts::karpenter_crd::KarpenterCrdChart;
use crate::cloud_provider::aws::kubernetes::eks::EKS;
use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::regions::AwsZone;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups};
use crate::cloud_provider::CloudProvider;
use crate::cmd::terraform_validators::TerraformValidators;
use crate::dns_provider::DnsProvider;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::{EventMessage, InfrastructureStep, Stage};
use crate::infrastructure_action::delete_kube_apps::delete_kube_apps;
use crate::infrastructure_action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure_action::eks::karpenter::node_groups_when_karpenter_is_enabled;
use crate::infrastructure_action::eks::karpenter::Karpenter;
use crate::infrastructure_action::eks::nodegroup::{
    delete_eks_nodegroups, should_update_desired_nodes, NodeGroupsDeletionType,
};
use crate::infrastructure_action::eks::tera_context::eks_tera_context;
use crate::infrastructure_action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure_action::eks::{AwsEksQoveryTerraformOutput, AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION};
use crate::infrastructure_action::InfraLogger;
use crate::runtime::block_on;
use crate::secret_manager::vault::QVaultClient;
use crate::services::kube_client::SelectK8sResourceBy;
use crate::utilities::envs_to_string;
use crate::{cmd, secret_manager};
use std::collections::HashSet;

pub fn delete_eks_cluster(
    infra_ctx: &InfrastructureContext,
    kubernetes: &EKS,
    cloud_provider: &dyn CloudProvider,
    dns_provider: &dyn DnsProvider,
    aws_zones: &[AwsZone],
    node_groups: &[NodeGroups],
    options: &Options,
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));

    logger.info("Preparing cluster deletion.");
    let aws_conn = cloud_provider
        .aws_sdk_client()
        .ok_or_else(|| Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details.clone())))?;

    let temp_dir = kubernetes.temp_dir();
    let node_groups = node_groups_when_karpenter_is_enabled(
        kubernetes,
        infra_ctx,
        node_groups,
        &event_details,
        KubernetesClusterAction::Delete,
    )?;

    let node_groups = should_update_desired_nodes(
        event_details.clone(),
        kubernetes,
        KubernetesClusterAction::Delete,
        node_groups,
        get_rusoto_eks_client(event_details.clone(), kubernetes, cloud_provider).ok(),
    )?;

    // generate terraform files and copy them into temp dir
    // in case error, this should no be a blocking error
    let mut cluster_upgrade_timeout_in_min = AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
    if let Ok(kube_client) = infra_ctx.mk_kube_client() {
        let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
            .unwrap_or(Vec::with_capacity(0));

        let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Delete);
        cluster_upgrade_timeout_in_min = timeout;
        if let Some(x) = message {
            logger.info(x);
        }
    }

    let mut tera_context = eks_tera_context(
        kubernetes,
        cloud_provider,
        dns_provider,
        aws_zones,
        &node_groups,
        options,
        cluster_upgrade_timeout_in_min,
        false,
        advanced_settings,
        qovery_allowed_public_access_cidrs,
    )?;
    tera_context.insert("is_deletion_step", &true);

    let tf_resources = TerraformInfraResources::new(
        tera_context.clone(),
        kubernetes.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        infra_ctx.context().is_dry_run_deploy(),
    );

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    let message = format!(
        "Ensuring everything is up to date before deleting cluster {}/{}",
        kubernetes.name(),
        kubernetes.short_id()
    );

    logger.info(message);
    logger.info("Running Terraform apply before running a delete.");

    let _: Result<AwsEksQoveryTerraformOutput, Box<EngineError>> = tf_resources.create(&logger).inspect_err(|e| {
        logger.warn(EventMessage::new(
            "Terraform apply before delete failed. It may occur but may not be blocking.".to_string(),
            Some(e.to_string()),
        ));
    });

    let skip_helm_release = if kubernetes.is_karpenter_enabled() {
        HashSet::from([
            KarpenterChart::chart_name(),
            KarpenterConfigurationChart::chart_name(),
            KarpenterCrdChart::chart_name(),
        ])
    } else {
        HashSet::with_capacity(0)
    };
    delete_kube_apps(kubernetes, infra_ctx, event_details.clone(), &logger, skip_helm_release)?;

    logger.info(format!(
        "Deleting Kubernetes cluster {}/{}",
        kubernetes.name(),
        kubernetes.short_id()
    ));
    if kubernetes.is_karpenter_enabled() {
        let kube_client = infra_ctx.mk_kube_client()?;
        block_on(Karpenter::delete(kubernetes, cloud_provider, &kube_client))?;
    } else {
        // remove all node groups to avoid issues because of nodegroups manually added by user, making terraform unable to delete the EKS cluster
        block_on(delete_eks_nodegroups(
            aws_conn,
            kubernetes.cluster_name(),
            kubernetes.context().is_first_cluster_deployment(),
            NodeGroupsDeletionType::All,
            event_details.clone(),
        ))?;
    }

    // remove S3 buckets from tf state
    // TODO: Why do we forgot them ?
    logger.info("Removing S3 buckets from tf state");
    let resources_to_be_removed_from_tf_state: Vec<(&str, &str)> = vec![
        ("aws_s3_bucket.loki_bucket", "S3 logs bucket"),
        ("aws_s3_bucket_lifecycle_configuration.loki_lifecycle", "S3 logs lifecycle"),
        ("aws_s3_bucket.vpc_flow_logs", "S3 flow logs bucket"),
        (
            "aws_s3_bucket_lifecycle_configuration.vpc_flow_logs_lifecycle",
            "S3 vpc log flow lifecycle",
        ),
    ];

    for resource_to_be_removed_from_tf_state in resources_to_be_removed_from_tf_state {
        match cmd::terraform::terraform_remove_resource_from_tf_state(
            temp_dir.join("terraform").to_string_lossy().as_ref(),
            resource_to_be_removed_from_tf_state.0,
            &TerraformValidators::None,
        ) {
            Ok(_) => {
                logger.info(format!(
                    "{} successfully removed from tf state.",
                    resource_to_be_removed_from_tf_state.1
                ));
            }
            Err(err) => {
                // We weren't able to remove S3 bucket from tf state, maybe it's not there?
                // Anyways, this is not blocking
                logger.warn(EventMessage::new_from_engine_error(EngineError::new_terraform_error(
                    event_details.clone(),
                    err,
                )));
            }
        }
    }

    logger.info("Running Terraform destroy");
    tf_resources.delete(&logger)?;

    logger.info("Kubernetes cluster successfully deleted");

    // delete info on vault
    if let Ok(vault_conn) = QVaultClient::new(event_details) {
        let mount = secret_manager::vault::get_vault_mount_name(kubernetes.context().is_test_cluster());
        // ignore on failure
        let _ = vault_conn.delete_secret(mount.as_str(), kubernetes.short_id());
    };

    Ok(())
}
