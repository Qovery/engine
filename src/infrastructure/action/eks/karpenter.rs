use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{Helm, to_engine_error};
use crate::cmd::kubectl::kubectl_exec_get_pods;
use crate::cmd::structs::KubernetesPodStatusPhase;
use crate::environment::models::ToCloudProviderFormat;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage};
use crate::helm::{ChartInfo, HelmChartError, HelmChartNamespaces};
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::eks::AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
use crate::infrastructure::action::eks::AwsEksQoveryTerraformOutput;
use crate::infrastructure::action::eks::helm_charts::karpenter::KarpenterChart;
use crate::infrastructure::action::eks::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use crate::infrastructure::action::eks::sdk::QoveryAwsSdkConfigEks;
use crate::infrastructure::action::eks::tera_context::eks_tera_context;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::infrastructure::models::kubernetes::aws::{AwsStorageType, Options};
use crate::io_models::models::{KubernetesClusterAction, NodeGroups};
use crate::runtime::block_on;
use crate::services::kube_client::{QubeClient, SelectK8sResourceBy};
use crate::utilities::envs_to_string;
use aws_types::SdkConfig;
use chrono::Duration as ChronoDuration;
use k8s_openapi::api::core::v1::{Node, Pod};
use retry::OperationResult;
use retry::delay::Fixed;
use std::path::PathBuf;
use std::str::FromStr;
use std::string::ToString;
use std::time::Duration;

const KARPENTER_NAMESPACE: &str = "kube-system";
const KARPENTER_LABEL_SELECTOR: &str = "app.kubernetes.io/instance=karpenter";
const KARPENTER_EXPECTED_POD_COUNT: u32 = 2;
const KARPENTER_DEPLOYMENT_NAME: &str = "karpenter";
const KARPENTER_POLLING_INTERVAL_SECS: u64 = 30;
const KARPENTER_NODE_WAIT_RETRIES: usize = 5;
const EC2NODECLASS_CLEANUP_MAX_RETRIES: usize = 20;
/// Overall timeout for the delete path (15 minutes).
const KARPENTER_DELETE_TIMEOUT: Duration = Duration::from_secs(900);
/// Overall timeout for the pause path (10 minutes).
const KARPENTER_PAUSE_TIMEOUT: Duration = Duration::from_secs(600);
/// Timeout for the karpenter-configuration chart uninstall during delete.
const KARPENTER_CHART_UNINSTALL_TIMEOUT: ChronoDuration = ChronoDuration::seconds(300);

// Terraform resources for karpenter nodegroup (used for pause/resume)
const KARPENTER_NODEGROUP_TERRAFORM_RESOURCES: &[&str] = &[
    "aws_launch_template.karpenter_nodegroup",
    "aws_eks_node_group.karpenter_controller",
];

pub struct Karpenter {}

impl Karpenter {
    pub async fn pause(
        kubernetes: &EKS,
        infra_ctx: &InfrastructureContext,
        client: &QubeClient,
        logger: &impl InfraLogger,
    ) -> Result<(), Box<EngineError>> {
        let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));

        Self::drain_karpenter_nodes(client, &event_details).await?;

        // scale down the karpenter deployment
        client
            .set_deployment_replicas_number(
                event_details.clone(),
                KARPENTER_DEPLOYMENT_NAME,
                &HelmChartNamespaces::KubeSystem.to_string(),
                0,
            )
            .await?;

        // delete the karpenter-controller nodegroup
        logger.info("Deleting karpenter-controller nodegroup...");
        Self::delete_karpenter_nodegroup(kubernetes, infra_ctx, logger)?;
        logger.info("Karpenter-controller nodegroup deleted successfully");

        Ok(())
    }

    fn delete_karpenter_nodegroup(
        kubernetes: &EKS,
        infra_ctx: &InfrastructureContext,
        logger: &impl InfraLogger,
    ) -> Result<(), Box<EngineError>> {
        let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));
        let cloud_provider = infra_ctx.cloud_provider();
        let dns_provider = infra_ctx.dns_provider();

        // Build terraform context normally
        let tera_context = eks_tera_context(
            kubernetes,
            cloud_provider,
            dns_provider,
            kubernetes.zones.as_slice(),
            &[], // No regular nodegroups
            &kubernetes.options,
            AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION,
            &kubernetes.advanced_settings,
            kubernetes.qovery_allowed_public_access_cidrs.as_ref(),
        )?;

        let tf_action = TerraformInfraResources::new(
            tera_context,
            PathBuf::from(&kubernetes.template_directory).join("terraform"),
            kubernetes.temp_dir().join("terraform"),
            event_details,
            envs_to_string(cloud_provider.credentials_environment_variables()),
            infra_ctx.context().is_dry_run_deploy(),
        );

        // Explicitly destroy the karpenter nodegroup resources
        tf_action.destroy_specific_resources(KARPENTER_NODEGROUP_TERRAFORM_RESOURCES, logger)?;

        logger.info("Karpenter nodegroup terraform resources deleted");
        Ok(())
    }

    pub async fn restart(
        kubernetes: &EKS,
        cloud_provider: &dyn CloudProvider,
        terraform_output: &AwsEksQoveryTerraformOutput,
        client: &QubeClient,
        kubernetes_long_id: uuid::Uuid,
        options: &Options,
    ) -> Result<(), Box<EngineError>> {
        let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Restart));

        // scale up the karpenter deployment
        client
            .set_deployment_replicas_number(
                event_details.clone(),
                KARPENTER_DEPLOYMENT_NAME,
                &HelmChartNamespaces::KubeSystem.to_string(),
                KARPENTER_EXPECTED_POD_COUNT,
            )
            .await?;

        Self::wait_for_karpenter_pods(kubernetes, cloud_provider, &event_details).await?;

        Self::install_karpenter_configuration(
            kubernetes,
            cloud_provider,
            terraform_output,
            &event_details,
            kubernetes_long_id,
            options,
        )
    }

    pub async fn delete(
        kubernetes: &EKS,
        cloud_provider: &dyn CloudProvider,
        client: &QubeClient,
    ) -> Result<(), Box<EngineError>> {
        let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));

        Self::delete_karpenter_nodes_for_cluster_deletion(kubernetes, cloud_provider, client, &event_details).await?;

        // uninstall Karpenter
        if let Err(e) = uninstall_chart(
            kubernetes,
            cloud_provider,
            &event_details,
            &KarpenterChart::chart_name(),
            &HelmChartNamespaces::KubeSystem.to_string(),
            None,
        ) {
            kubernetes
                .logger()
                .log(EngineEvent::Warning(event_details.clone(), EventMessage::from(*e)));
        }

        Ok(())
    }

    pub fn is_paused(kube_client: &QubeClient, event_details: &EventDetails) -> Result<bool, Box<EngineError>> {
        if !Self::deployment_is_installed(kube_client, event_details) {
            return Ok(false);
        }

        let nodes = block_on(Self::get_nodes_spawned_by_karpenter(kube_client, event_details))?;
        Ok(nodes.is_empty())
    }

    pub fn deployment_is_installed(kube_client: &QubeClient, event_details: &EventDetails) -> bool {
        let deployments = block_on(kube_client.get_deployments(
            event_details.clone(),
            Some(&HelmChartNamespaces::KubeSystem.to_string()),
            SelectK8sResourceBy::LabelsSelector("app.kubernetes.io/name=karpenter".to_string()),
        ))
        .unwrap_or(Vec::with_capacity(0));

        !deployments.is_empty()
    }

    pub async fn create_aws_service_role_for_ec2_spot(
        aws_conn: &SdkConfig,
        event_details: &EventDetails,
    ) -> Result<(), Box<EngineError>> {
        match aws_conn.get_role("AWSServiceRoleForEC2Spot").await {
            Ok(_) => Ok(()),
            Err(_) => Ok(aws_conn
                .create_service_linked_role("spot.amazonaws.com")
                .await
                .map(|_| ())
                .map_err(|e| {
                    EngineError::new_cannot_create_aws_service_linked_role_for_spot_instance(
                        event_details.clone(),
                        CommandError::new(
                            "Fail to create the service-linked role: AWSServiceRoleForEC2Spot".to_string(),
                            Some(e.to_string()),
                            None,
                        ),
                    )
                })?),
        }
    }

    async fn get_nodes_spawned_by_karpenter(
        client: &QubeClient,
        event_details: &EventDetails,
    ) -> Result<Vec<Node>, Box<EngineError>> {
        client
            .get_nodes(
                event_details.clone(),
                SelectK8sResourceBy::LabelsSelector("karpenter.sh/nodepool".to_string()),
            )
            .await
    }

    /// Drain path for cluster **pause**: cordon nodes, delete evictable pods, delete NodePools,
    /// then wait for NodeClaims and nodes to disappear. Skips chart uninstall and EC2NodeClass
    /// verification (resume will reinstall the chart, and EC2NodeClasses are harmless during pause).
    /// Wrapped in an overall timeout as a safety net.
    async fn drain_karpenter_nodes(client: &QubeClient, event_details: &EventDetails) -> Result<(), Box<EngineError>> {
        let result = tokio::time::timeout(KARPENTER_PAUSE_TIMEOUT, async {
            // Step 1: Early exit if no Karpenter nodes exist
            let nodes = Self::get_nodes_spawned_by_karpenter(client, event_details).await?;
            if nodes.is_empty() {
                return Ok(());
            }

            // Step 2: Cordon all Karpenter nodes to prevent pod rescheduling back to them
            Self::cordon_karpenter_nodes(client, event_details, &nodes).await;

            // Step 3: Delete all non-DaemonSet pods on Karpenter nodes.
            // This empties the nodes before triggering NodePool deletion, so Karpenter's
            // drain has little left to evict — avoiding the double-wait pattern.
            Self::delete_evictable_pods_on_karpenter_nodes(client, event_details, &nodes).await;

            // Step 4: Delete NodePool CRs to trigger Karpenter drain on near-empty nodes
            Self::delete_all_node_pools(client, event_details).await?;

            // Step 5: Wait for NodeClaims to be fully deleted
            Self::wait_for_node_claims_deletion(client, event_details, KARPENTER_NODE_WAIT_RETRIES).await;

            // Step 6: Wait for Karpenter-spawned nodes to be gone
            Self::wait_for_karpenter_nodes_deletion(client, event_details).await;

            Ok(())
        })
        .await;

        match result {
            Ok(inner) => inner,
            Err(_) => {
                warn!(
                    "Karpenter node drain timed out after {}s during pause",
                    KARPENTER_PAUSE_TIMEOUT.as_secs()
                );
                Ok(())
            }
        }
    }

    /// Cleanup path for cluster **delete**: cordon nodes, delete evictable pods, delete NodePools,
    /// wait for NodeClaims/nodes, uninstall karpenter-configuration chart, and verify EC2NodeClasses
    /// are cleaned up. Wrapped in an overall timeout as a safety net.
    async fn delete_karpenter_nodes_for_cluster_deletion(
        kubernetes: &EKS,
        cloud_provider: &dyn CloudProvider,
        client: &QubeClient,
        event_details: &EventDetails,
    ) -> Result<(), Box<EngineError>> {
        let result = tokio::time::timeout(KARPENTER_DELETE_TIMEOUT, async {
            // Step 1: Early exit if no Karpenter nodes exist
            let _karpenter_parameters = kubernetes.get_karpenter_parameters().ok_or_else(|| {
                Box::new(EngineError::new_k8s_delete_karpenter_nodes_error(
                    event_details.clone(),
                    CommandError::new_from_safe_message("Karpenter parameters are missing".to_string()),
                ))
            })?;

            let nodes = Self::get_nodes_spawned_by_karpenter(client, event_details).await?;
            if nodes.is_empty() {
                return Ok(());
            }

            // Step 2: Cordon all Karpenter nodes to prevent pod rescheduling back to them
            Self::cordon_karpenter_nodes(client, event_details, &nodes).await;

            // Step 3: Delete all non-DaemonSet pods on Karpenter nodes
            Self::delete_evictable_pods_on_karpenter_nodes(client, event_details, &nodes).await;

            // Step 4: Delete NodePool CRs to trigger Karpenter drain on near-empty nodes
            Self::delete_all_node_pools(client, event_details).await?;

            // Step 5: Wait for NodeClaims to be fully deleted
            Self::wait_for_node_claims_deletion(client, event_details, KARPENTER_NODE_WAIT_RETRIES).await;

            // Step 6: Wait for Karpenter-spawned nodes to be gone
            Self::wait_for_karpenter_nodes_deletion(client, event_details).await;

            // Step 7: Uninstall karpenter-configuration chart.
            // At this point only EC2NodeClasses remain (no NodeClaims referencing them).
            if let Err(e) = uninstall_chart(
                kubernetes,
                cloud_provider,
                event_details,
                &KarpenterConfigurationChart::chart_name(),
                &HelmChartNamespaces::KubeSystem.to_string(),
                Some(KARPENTER_CHART_UNINSTALL_TIMEOUT),
            ) {
                kubernetes
                    .logger()
                    .log(EngineEvent::Warning(event_details.clone(), EventMessage::from(*e)));
            }

            // Step 8: Verify EC2NodeClasses are cleaned up
            Self::wait_for_ec2_node_classes_cleanup(client, event_details).await
        })
        .await;

        match result {
            Ok(inner) => inner,
            Err(_) => {
                warn!(
                    "Karpenter node cleanup timed out after {}s during delete",
                    KARPENTER_DELETE_TIMEOUT.as_secs()
                );
                Ok(())
            }
        }
    }

    /// Cordons all provided Karpenter nodes by setting `spec.unschedulable = true`.
    /// Failures are logged as warnings but do not abort the cleanup process.
    async fn cordon_karpenter_nodes(client: &QubeClient, event_details: &EventDetails, nodes: &[Node]) {
        info!("Cordoning {} Karpenter node(s)...", nodes.len());

        for node in nodes {
            let node_name = match node.metadata.name.as_deref() {
                Some(name) => name,
                None => continue,
            };

            match client.cordon_node(event_details.clone(), node.clone()).await {
                Ok(()) => info!("Cordoned node '{}'", node_name),
                Err(e) => warn!("Failed to cordon node '{}': {}", node_name, e),
            }
        }
    }

    /// Deletes all non-DaemonSet pods on the provided Karpenter nodes.
    /// This clears the nodes before triggering NodePool deletion, so Karpenter's
    /// drain encounters near-empty nodes and completes in a single wait cycle.
    async fn delete_evictable_pods_on_karpenter_nodes(
        client: &QubeClient,
        event_details: &EventDetails,
        nodes: &[Node],
    ) {
        info!(
            "Deleting evictable pods on {} Karpenter node(s) to speed up drain...",
            nodes.len()
        );

        for node in nodes {
            let node_name = match node.metadata.name.as_deref() {
                Some(name) => name,
                None => continue,
            };

            let pods = match client.get_pods_on_node(event_details, node_name).await {
                Ok(pods) => pods,
                Err(e) => {
                    warn!("Error listing pods on node '{}': {}", node_name, e);
                    continue;
                }
            };

            for pod in &pods {
                if !is_evictable_pod(pod) {
                    continue;
                }

                let pod_name = pod.metadata.name.as_deref().unwrap_or("<unknown>");
                let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");

                match client.delete_pod(event_details, namespace, pod_name).await {
                    Ok(()) => info!("Deleted pod '{}/{}' from node '{}'", namespace, pod_name, node_name),
                    Err(e) => warn!(
                        "Failed to delete pod '{}/{}' from node '{}': {}",
                        namespace, pod_name, node_name, e
                    ),
                }
            }
        }
    }

    async fn delete_all_node_pools(client: &QubeClient, event_details: &EventDetails) -> Result<(), Box<EngineError>> {
        info!("Deleting Karpenter NodePools to trigger node drain...");
        let node_pools = client.get_node_pools(event_details).await?;

        for node_pool in &node_pools {
            let name = node_pool.metadata.name.as_deref().unwrap_or("<unknown>");
            match client.delete_node_pool(event_details, name).await {
                Ok(_) => info!("NodePool '{}' deletion requested", name),
                Err(e) => warn!(
                    "Failed to delete NodePool '{}', will be cleaned up by chart uninstall: {}",
                    name,
                    e.message(ErrorMessageVerbosity::FullDetails)
                ),
            }
        }

        Ok(())
    }

    async fn wait_for_node_claims_deletion(client: &QubeClient, event_details: &EventDetails, max_retries: usize) {
        info!(
            "Waiting for NodeClaims to be deleted (max {} retries, {}s interval)...",
            max_retries, KARPENTER_POLLING_INTERVAL_SECS
        );

        for retry in 0..max_retries {
            match client.get_node_claims(event_details).await {
                Ok(items) if items.is_empty() => {
                    info!("All NodeClaims have been deleted");
                    return;
                }
                Ok(items) => {
                    info!(
                        "Waiting for {} NodeClaim(s) to be deleted (retry {}/{})...",
                        items.len(),
                        retry + 1,
                        max_retries
                    );
                }
                Err(e) => {
                    warn!("Error when trying to get NodeClaims: {}", e);
                }
            }
            tokio::time::sleep(Duration::from_secs(KARPENTER_POLLING_INTERVAL_SECS)).await;
        }

        warn!("Timed out waiting for NodeClaims to be deleted after {} retries", max_retries);
    }

    async fn wait_for_karpenter_nodes_deletion(client: &QubeClient, event_details: &EventDetails) {
        for retry in 0..KARPENTER_NODE_WAIT_RETRIES {
            match Self::get_nodes_spawned_by_karpenter(client, event_details).await {
                Ok(nodes) if nodes.is_empty() => {
                    info!("All Karpenter-spawned nodes are gone");
                    return;
                }
                Ok(nodes) => {
                    info!(
                        "Waiting for {} Karpenter node(s) to be removed (retry {}/{})...",
                        nodes.len(),
                        retry + 1,
                        KARPENTER_NODE_WAIT_RETRIES
                    );
                }
                Err(e) => {
                    warn!("Error when trying to get Karpenter nodes: {}", e);
                }
            }
            tokio::time::sleep(Duration::from_secs(KARPENTER_POLLING_INTERVAL_SECS)).await;
        }

        warn!(
            "Timed out waiting for Karpenter nodes to disappear after {} retries",
            KARPENTER_NODE_WAIT_RETRIES
        );
    }

    async fn wait_for_ec2_node_classes_cleanup(
        client: &QubeClient,
        event_details: &EventDetails,
    ) -> Result<(), Box<EngineError>> {
        for retry in 0..EC2NODECLASS_CLEANUP_MAX_RETRIES {
            match client.get_ec2_node_classes(event_details).await {
                Ok(items) if items.is_empty() => return Ok(()),
                Ok(items) => {
                    info!(
                        "Waiting for {} EC2NodeClass(es) to be cleaned up (retry {}/{})...",
                        items.len(),
                        retry + 1,
                        EC2NODECLASS_CLEANUP_MAX_RETRIES
                    );
                }
                Err(e) => {
                    warn!("Error when trying to get EC2NodeClass: {}", e);
                }
            }
            tokio::time::sleep(Duration::from_secs(KARPENTER_POLLING_INTERVAL_SECS)).await;
        }

        // Final check after all retries
        let ec2_node_classes = client.get_ec2_node_classes(event_details).await?;
        if !ec2_node_classes.is_empty() {
            return Err(Box::new(EngineError::new_nodegroup_delete_error(
                event_details.clone(),
                Some("Karpenter".to_string()),
                "can't delete nodes spawned by Karpenter".to_string(),
            )));
        }

        Ok(())
    }

    fn install_karpenter_configuration(
        kubernetes: &EKS,
        cloud_provider: &dyn CloudProvider,
        terraform_output: &AwsEksQoveryTerraformOutput,
        event_details: &EventDetails,
        cluster_long_id: uuid::Uuid,
        options: &Options,
    ) -> Result<(), Box<EngineError>> {
        let kubernetes_config_file_path = kubernetes.kubeconfig_local_file_path();
        let helm = Helm::new(
            Some(kubernetes_config_file_path),
            &cloud_provider.credentials_environment_variables(),
        )
        .map_err(|e| to_engine_error(event_details, e))?;

        let karpenter_configuration_chart = Self::get_karpenter_configuration_chart(
            kubernetes,
            terraform_output,
            cluster_long_id,
            event_details,
            options,
        )?;

        Ok(helm
            .upgrade(&karpenter_configuration_chart, &[], &CommandKiller::never())
            .map_err(|e| {
                EngineError::new_helm_charts_upgrade_error(
                    event_details.clone(),
                    CommandError::new(
                        "can't upgrade helm karpenter-configuration".to_string(),
                        Some(e.to_string()),
                        None,
                    ),
                )
            })?)
    }

    fn get_karpenter_configuration_chart(
        kubernetes: &EKS,
        terraform_output: &AwsEksQoveryTerraformOutput,
        cluster_long_id: uuid::Uuid,
        event_details: &EventDetails,
        options: &Options,
    ) -> Result<ChartInfo, Box<EngineError>> {
        let karpenter_parameters = kubernetes.get_karpenter_parameters().ok_or_else(|| {
            Box::new(EngineError::new_k8s_delete_karpenter_nodes_error(
                event_details.clone(),
                CommandError::new_from_safe_message("Karpenter parameters are missing".to_string()),
            ))
        })?;

        let cluster_id = kubernetes.short_id().to_string();
        let region = AwsRegion::from_str(kubernetes.region()).map_err(|_e| {
            EngineError::new_unsupported_region(event_details.clone(), kubernetes.region().to_string(), None)
        })?;
        let cluster_name = kubernetes.cluster_name();

        // Karpenter Configuration
        let mut karpenter_configuration_chart = KarpenterConfigurationChart::new(
            Some(kubernetes.temp_dir().to_string_lossy().as_ref()),
            cluster_name.to_string(),
            true,
            terraform_output.cluster_security_group_id.clone(),
            &cluster_id,
            cluster_long_id,
            kubernetes.context.organization_short_id(),
            *kubernetes.context.organization_long_id(),
            kubernetes.version.clone(),
            region.to_cloud_provider_format(),
            karpenter_parameters,
            options.user_provided_network.as_ref(),
            kubernetes.advanced_settings().aws_eks_ec2_ami.to_model(),
            AwsStorageType::try_from(kubernetes.advanced_settings.k8s_storage_class_fast_ssd.to_model()).map_err(
                |e| {
                    Box::new(EngineError::new_k8s_delete_karpenter_nodes_error(
                        event_details.clone(),
                        CommandError::new(
                            format!(
                                "Unknown AWS Storage type `{}`",
                                kubernetes.advanced_settings.k8s_storage_class_fast_ssd
                            ),
                            Some(e.to_string()),
                            None,
                        ),
                    ))
                },
            )?,
            kubernetes.advanced_settings().pleco_resources_ttl,
            options.resource_tags.clone(),
        )
        .to_common_helm_chart()
        .map_err(|el| {
            EngineError::new_helm_charts_setup_error(
                event_details.clone(),
                CommandError::new(
                    "Error while create karpenter-configuration chart".to_string(),
                    Some(el.to_string()),
                    None,
                ),
            )
        })?;

        // Override the path to the chart, as it as not been rendered on disk during the normal chart flow
        // we take it directly from the template directory
        karpenter_configuration_chart.chart_info.path = kubernetes
            .template_directory
            .join("charts")
            .join(karpenter_configuration_chart.chart_info.name.clone())
            .to_string_lossy()
            .to_string();
        karpenter_configuration_chart.chart_info.values_files = vec![];

        Ok(karpenter_configuration_chart.chart_info)
    }

    async fn wait_for_karpenter_pods(
        kubernetes: &dyn Kubernetes,
        cloud_provider: &dyn CloudProvider,
        event_details: &EventDetails,
    ) -> Result<(), Box<EngineError>> {
        retry::retry(Fixed::from(Duration::from_secs(10)).take(10), || {
            match kubectl_exec_get_pods(
                kubernetes.kubeconfig_local_file_path(),
                Some(KARPENTER_NAMESPACE),
                Some(KARPENTER_LABEL_SELECTOR),
                cloud_provider.credentials_environment_variables(),
            ) {
                Ok(res) => {
                    let running_pods_count = res
                        .items
                        .iter()
                        .filter(|pod| pod.status.phase == KubernetesPodStatusPhase::Running)
                        .count();

                    if running_pods_count == KARPENTER_EXPECTED_POD_COUNT as usize {
                        OperationResult::Ok(())
                    } else {
                        OperationResult::Retry(CommandError::new_from_safe_message(
                            "Pods didn't restart yet. Waiting...".to_string(),
                        ))
                    }
                }
                Err(e) => OperationResult::Retry(e),
            }
        })
        .map_err(|e| {
            Box::new(EngineError::new_k8s_cannot_get_pods(
                event_details.clone(),
                CommandError::new_from_safe_message(format!("Error while trying to scale up Karpenter: {e}")),
            ))
        })
    }
}

/// Returns `true` if the pod can be evicted (is not owned by a DaemonSet).
/// DaemonSet pods are managed by the DaemonSet controller and will be recreated on the
/// same node, so deleting them would be counterproductive.
fn is_evictable_pod(pod: &Pod) -> bool {
    let owned_by_daemonset = pod
        .metadata
        .owner_references
        .as_ref()
        .is_some_and(|refs| refs.iter().any(|r| r.kind == "DaemonSet"));
    !owned_by_daemonset
}

fn uninstall_chart(
    kubernetes: &dyn Kubernetes,
    cloud_provider: &dyn CloudProvider,
    event_details: &EventDetails,
    chart_name: &str,
    chart_namespace: &str,
    uninstall_timeout: Option<ChronoDuration>,
) -> Result<(), Box<EngineError>> {
    let kubernetes_config_file_path = kubernetes.kubeconfig_local_file_path();

    let helm = Helm::new(
        Some(kubernetes_config_file_path),
        &cloud_provider.credentials_environment_variables(),
    )
    .map_err(|e| to_engine_error(event_details, e))?;

    let mut chart = ChartInfo::new_from_release_name(chart_name, chart_namespace);
    if let Some(timeout) = uninstall_timeout {
        chart.timeout_in_seconds = timeout.num_seconds();
    }

    helm.uninstall(&chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {})
        .map_err(|err| {
            Box::new(EngineError::new_helm_chart_error(
                event_details.clone(),
                HelmChartError::HelmError(err),
            ))
        })
}

pub fn node_groups_when_karpenter_is_enabled<'a>(
    kubernetes: &dyn Kubernetes,
    infra_context: &InfrastructureContext,
    node_groups: &'a [NodeGroups],
    event_details: &EventDetails,
    kubernetes_action: KubernetesClusterAction,
) -> Result<&'a [NodeGroups], Box<EngineError>> {
    if !kubernetes.is_karpenter_enabled() {
        return Ok(node_groups);
    }

    match kubernetes_action {
        KubernetesClusterAction::Bootstrap
        | KubernetesClusterAction::Upgrade(_)
        | KubernetesClusterAction::Pause
        | KubernetesClusterAction::Resume(_)
        | KubernetesClusterAction::Delete
        | KubernetesClusterAction::CleanKarpenterMigration => Ok(&[]),
        KubernetesClusterAction::Update(_)
            if infra_context.context().is_first_cluster_deployment()
                || Karpenter::deployment_is_installed(&infra_context.mk_kube_client()?, event_details) =>
        {
            Ok(&[])
        }
        KubernetesClusterAction::Update(_) => Ok(node_groups),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::Pod;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;

    fn pod_with_owner(kind: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: Some(vec![OwnerReference {
                    kind: kind.to_string(),
                    name: "owner".to_string(),
                    api_version: "v1".to_string(),
                    uid: "uid-123".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn pod_without_owner() -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some("standalone-pod".to_string()),
                namespace: Some("default".to_string()),
                owner_references: None,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn daemonset_pod_is_not_evictable() {
        assert!(!is_evictable_pod(&pod_with_owner("DaemonSet")));
    }

    #[test]
    fn replicaset_pod_is_evictable() {
        assert!(is_evictable_pod(&pod_with_owner("ReplicaSet")));
    }

    #[test]
    fn statefulset_pod_is_evictable() {
        assert!(is_evictable_pod(&pod_with_owner("StatefulSet")));
    }

    #[test]
    fn job_pod_is_evictable() {
        assert!(is_evictable_pod(&pod_with_owner("Job")));
    }

    #[test]
    fn standalone_pod_is_evictable() {
        assert!(is_evictable_pod(&pod_without_owner()));
    }
}
