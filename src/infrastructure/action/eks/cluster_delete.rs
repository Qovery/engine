use super::helm_charts::karpenter::KarpenterChart;
use super::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use super::helm_charts::karpenter_crd::KarpenterCrdChart;
use crate::errors::EngineError;
use crate::events::{EventMessage, InfrastructureStep, Stage};
use crate::infrastructure::action::delete_kube_apps::delete_kube_apps;
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::eks::karpenter::node_groups_when_karpenter_is_enabled;
use crate::infrastructure::action::eks::karpenter::Karpenter;
use crate::infrastructure::action::eks::nodegroup::{
    delete_eks_nodegroups, should_update_desired_nodes, NodeGroupsDeletionType,
};
use crate::infrastructure::action::eks::tera_context::eks_tera_context;
use crate::infrastructure::action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure::action::eks::{AwsEksQoveryTerraformOutput, AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION};
use crate::infrastructure::action::kubeconfig_helper::update_kubeconfig_file;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::aws::regions::AwsZone;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::dns_provider::DnsProvider;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::infrastructure::models::kubernetes::aws::Options;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::models::{KubernetesClusterAction, NodeGroups};
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use crate::utilities::envs_to_string;
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
        kubernetes.temp_dir.join("terraform"),
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

    let tf_output: Result<AwsEksQoveryTerraformOutput, Box<EngineError>> = tf_resources.create(&logger);
    match tf_output {
        Ok(tf_output) => {
            update_kubeconfig_file(kubernetes, &tf_output.kubeconfig)?;
        }
        Err(e) => {
            logger.warn(EventMessage::new(
                "Terraform apply before delete failed. It may occur but may not be blocking.".to_string(),
                Some(e.to_string()),
            ));
        }
    }

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
            NodeGroupsDeletionType::All,
            event_details.clone(),
        ))?;
    }

    // remove S3 logs buckets from tf state
    // Because deleting them inside terraform often lead to a timeout
    // so we delegate the responsibility to delete them to the user
    let resources_to_be_removed_from_tf_state: &[&str] = &[
        "aws_s3_bucket.loki_bucket",
        "aws_s3_bucket_lifecycle_configuration.loki_lifecycle",
        "aws_s3_bucket.vpc_flow_logs",
        "aws_s3_bucket_lifecycle_configuration.vpc_flow_logs_lifecycle",
    ];
    tf_resources.delete(resources_to_be_removed_from_tf_state, &logger)?;

    logger.info("Kubernetes cluster successfully deleted");

    Ok(())
}
