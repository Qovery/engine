use crate::cmd::command::CommandKiller;
use crate::environment::models::ToCloudProviderFormat;
use crate::errors::{CommandError, EngineError};
use crate::events::{InfrastructureStep, Stage};
use crate::helm::HelmChart;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::eks::AwsEksQoveryTerraformOutput;
use crate::infrastructure::action::eks::helm_charts::karpenter::KarpenterChart;
use crate::infrastructure::action::eks::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use crate::infrastructure::action::eks::helm_charts::karpenter_crd::KarpenterCrdChart;
use crate::infrastructure::action::eks::tera_context::eks_tera_context;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::io_models::context::Features;
use crate::services::kube_client::QubeClient;
use crate::utilities::envs_to_string;

const KARPENTER_NODEGROUP_TERRAFORM_RESOURCES: &[&str] = &[
    // Nodegroup resources
    "aws_launch_template.karpenter_nodegroup",
    "aws_eks_node_group.karpenter_controller",
    // IAM access entry for Karpenter nodes
    "aws_eks_access_entry.qovery_karpenter_access_entry",
    // IAM policy for Karpenter controller (needs update to include SQS permissions)
    "aws_iam_role_policy.karpenter_controller",
    // SQS queue for interruption handling
    "aws_sqs_queue.qovery-eks-queue",
    "aws_sqs_queue_policy.qovery_sqs_queue_policy",
    // CloudWatch events for spot interruptions (for_each resources)
    "aws_cloudwatch_event_rule.qovery_cloudwatch_event_rule",
    "aws_cloudwatch_event_target.qovery_cloudwatch_event_target",
];

/// Deploys Karpenter nodegroup infrastructure via Terraform and installs Karpenter helm charts.
///
/// This creates a dedicated nodegroup for Karpenter controller pods,
/// replacing Fargate-based deployment, and then installs:
/// 1. Karpenter CRDs
/// 2. Karpenter controller
/// 3. Karpenter configuration (node pools)
pub fn deploy_karpenter_nodegroup(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    logger: &impl InfraLogger,
) -> Result<AwsEksQoveryTerraformOutput, Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
    let cloud_provider = infra_ctx.cloud_provider();
    let dns_provider = infra_ctx.dns_provider();

    logger.info("🚀 Creating dedicated nodegroup for Karpenter controller (Fargate → Nodegroup migration)");

    // Build terraform context
    let tera_context = eks_tera_context(
        kubernetes,
        cloud_provider,
        dns_provider,
        kubernetes.zones.as_slice(),
        &[], // No regular nodegroups, only Karpenter nodegroup will be created
        &kubernetes.options,
        crate::infrastructure::action::eks::AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION,
        &kubernetes.advanced_settings,
        kubernetes.qovery_allowed_public_access_cidrs.as_ref(),
    )?;

    let tf_action = TerraformInfraResources::new(
        tera_context.clone(),
        kubernetes.template_directory.join("terraform"),
        kubernetes.temp_dir.join("terraform"), // SAME directory as main terraform for shared state
        event_details.clone(),
        envs_to_string(cloud_provider.credentials_environment_variables()),
        infra_ctx.context().is_dry_run_deploy(),
    );

    // Apply only the Karpenter nodegroup resources (launch template + nodegroup)
    logger.info("⚙️  Applying Terraform for Karpenter nodegroup...");
    tf_action.apply_specific_resources(KARPENTER_NODEGROUP_TERRAFORM_RESOURCES, logger)?;

    // Get terraform outputs
    let eks_tf_output: AwsEksQoveryTerraformOutput = tf_action.output()?;

    logger.info("✅ Karpenter nodegroup deployment completed successfully");

    // Install Karpenter helm charts
    logger.info("📦 Installing Karpenter helm charts...");
    install_karpenter_charts(kubernetes, infra_ctx, &eks_tf_output, logger)?;
    logger.info("✅ Karpenter helm charts installed successfully");

    Ok(eks_tf_output)
}

/// Installs Karpenter helm charts (CRDs, controller, and configuration).
///
/// Charts are installed sequentially:
/// 1. Karpenter CRDs - Required before controller installation (with automatic CRD verification)
/// 2. Karpenter controller - The main Karpenter controller
/// 3. Karpenter configuration - Node pools and settings
fn install_karpenter_charts(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    eks_tf_output: &AwsEksQoveryTerraformOutput,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
    let cloud_provider = infra_ctx.cloud_provider();
    let karpenter_parameters = kubernetes.get_karpenter_parameters().ok_or_else(|| {
        Box::new(EngineError::new_helm_chart_error(
            event_details.clone(),
            CommandError::new_from_safe_message(
                "Karpenter parameters should be present when installing Karpenter charts".to_string(),
            )
            .into(),
        ))
    })?;

    // Create Kubernetes client for CRD verification and chart operations
    let credentials_env_vars = envs_to_string(cloud_provider.credentials_environment_variables());
    let kube_client = QubeClient::new(
        event_details.clone(),
        Some(kubernetes.kubeconfig_local_file_path().to_path_buf()),
        credentials_env_vars.clone(),
    )?;

    // Setup environment variables for helm operations
    let envs: Vec<(&str, &str)> = credentials_env_vars
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    // Get chart prefix path from template directory
    let chart_prefix_path = kubernetes.template_directory.to_str();
    let kubeconfig_path = kubernetes.kubeconfig_local_file_path();

    // 1. Install Karpenter CRDs first (with automatic verification)
    logger.info("📦 Installing Karpenter CRDs...");
    let karpenter_crd_chart = KarpenterCrdChart::new(chart_prefix_path)
        .to_common_helm_chart()
        .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))?;

    karpenter_crd_chart
        .run(&kube_client, &kubeconfig_path, &envs, &CommandKiller::never())
        .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))?;
    logger.info("✅ Karpenter CRDs installed and verified");

    // 2. Install Karpenter controller
    logger.info("📦 Installing Karpenter controller...");
    let metrics_enabled = kubernetes.context().is_feature_enabled(&Features::MetricsHistory);
    let karpenter_chart = KarpenterChart::new(
        chart_prefix_path,
        kubernetes.cluster_name().to_string(),
        eks_tf_output.karpenter_controller_aws_role_arn.clone(),
        true, // replace_cluster_autoscaler
        metrics_enabled,
        false, // recreate_pods
    )
    .to_common_helm_chart()
    .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))?;

    karpenter_chart
        .run(&kube_client, &kubeconfig_path, &envs, &CommandKiller::never())
        .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))?;
    logger.info("✅ Karpenter controller installed");

    // 3. Install Karpenter configuration (node pools)
    logger.info("📦 Installing Karpenter configuration...");
    let aws_storage_type = crate::infrastructure::models::kubernetes::aws::AwsStorageType::try_from(
        kubernetes.advanced_settings.k8s_storage_class_fast_ssd.to_model(),
    )
    .map_err(|e| {
        Box::new(EngineError::new_helm_chart_error(
            event_details.clone(),
            CommandError::new_from_safe_message(format!("Unknown AWS Storage type: {e}")).into(),
        ))
    })?;

    let karpenter_configuration_chart = KarpenterConfigurationChart::new(
        chart_prefix_path,
        kubernetes.cluster_name().to_string(),
        true, // replace_cluster_autoscaler
        eks_tf_output.cluster_security_group_id.clone(),
        kubernetes.short_id(),
        kubernetes.long_id,
        kubernetes.context.organization_short_id(),
        *kubernetes.context.organization_long_id(),
        kubernetes.version().clone(),
        kubernetes.region.to_cloud_provider_format(),
        karpenter_parameters,
        kubernetes.options.user_provided_network.as_ref(),
        kubernetes.advanced_settings.aws_eks_ec2_ami.to_model(),
        aws_storage_type,
        kubernetes.advanced_settings.pleco_resources_ttl,
    )
    .to_common_helm_chart()
    .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))?;

    karpenter_configuration_chart
        .run(&kube_client, &kubeconfig_path, &envs, &CommandKiller::never())
        .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))?;
    logger.info("✅ Karpenter configuration installed");

    Ok(())
}

/// Determines if Karpenter nodegroup deployment should be performed
///
/// Returns true if:
/// 1. Karpenter is enabled
/// 2. This is not the first cluster deployment (migration scenario)
/// 3. The karpenter nodegroup is NOT already deployed (checked by verifying nodes and deployment)
///
/// Returns false if:
/// - Karpenter is not enabled
/// - This is the first cluster deployment
/// - The karpenter nodegroup is already deployed (exactly 2 nodes and 2 ready deployment replicas exist)
/// - Kubernetes API calls fail (safe default to avoid unnecessary deployments)
pub fn should_deploy_karpenter_nodegroup(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    logger: &impl InfraLogger,
) -> bool {
    // Check prerequisites
    let has_karpenter = kubernetes.get_karpenter_parameters().is_some();
    let is_first_deployment = infra_ctx.context().is_first_cluster_deployment();

    if !has_karpenter || is_first_deployment {
        logger.info("Skipping Karpenter migration: prerequisites not met");
        return false;
    }

    // Check if migration is already complete by querying cluster state
    let kube_client = match infra_ctx.mk_kube_client() {
        Ok(client) => client,
        Err(e) => {
            logger.warn(format!(
                "Cannot check Karpenter migration status: failed to create Kubernetes client: {e:?}"
            ));
            return false;
        }
    };

    let event_details =
        kubernetes.get_event_details(crate::events::Stage::Infrastructure(crate::events::InfrastructureStep::Create));

    // Check for karpenter nodes with infrastructure label
    let nodes_with_infra_label = match crate::runtime::block_on(kube_client.get_nodes(
        event_details.clone(),
        crate::services::kube_client::SelectK8sResourceBy::LabelsSelector(
            "node.qovery.com/infrastructure=true".to_string(),
        ),
    )) {
        Ok(nodes) => nodes,
        Err(e) => {
            logger.warn(format!("Cannot check Karpenter migration status: failed to query nodes: {e:?}"));
            return false;
        }
    };

    // Filter nodes by nodegroup name pattern (eks.amazonaws.com/nodegroup contains "karpenter-controller")
    let karpenter_nodes: Vec<_> = nodes_with_infra_label
        .iter()
        .filter(|node| {
            node.metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("eks.amazonaws.com/nodegroup"))
                .map(|ng| ng.contains("karpenter-controller"))
                .unwrap_or(false)
        })
        .collect();

    let node_count = karpenter_nodes.len();

    // Check for karpenter deployment in kube-system namespace
    let karpenter_deployments = match crate::runtime::block_on(kube_client.get_deployments(
        event_details.clone(),
        Some("kube-system"),
        crate::services::kube_client::SelectK8sResourceBy::Name("karpenter".to_string()),
    )) {
        Ok(deployments) => deployments,
        Err(e) => {
            logger.warn(format!(
                "Cannot check Karpenter migration status: failed to query karpenter deployment: {e:?}"
            ));
            return false;
        }
    };

    // Get ready replicas from deployment status
    let ready_replicas = karpenter_deployments
        .first()
        .and_then(|d| d.status.as_ref())
        .and_then(|s| s.ready_replicas)
        .unwrap_or(0);

    // Migration is complete if exactly 2 nodes and exactly 2 ready replicas exist
    let migration_complete = node_count == 2 && ready_replicas == 2;
    let should_migrate = !migration_complete;

    logger.info(format!(
        "Karpenter migration check: {node_count} nodes, {ready_replicas} replicas ready, migration needed: {should_migrate}"
    ));

    should_migrate
}
