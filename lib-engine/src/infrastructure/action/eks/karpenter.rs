use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{Helm, to_engine_error};
use crate::cmd::kubectl::kubectl_exec_get_pods;
use crate::cmd::structs::KubernetesPodStatusPhase;
use crate::environment::models::ToCloudProviderFormat;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage};
use crate::helm::{ChartInfo, HelmChartError, HelmChartNamespaces};
use crate::infrastructure::action::eks::AwsEksQoveryTerraformOutput;
use crate::infrastructure::action::eks::helm_charts::karpenter::KarpenterChart;
use crate::infrastructure::action::eks::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use crate::infrastructure::action::eks::sdk::QoveryAwsSdkConfigEks;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::aws::Options;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::io_models::models::{KubernetesClusterAction, NodeGroups};
use crate::runtime::block_on;
use crate::services::kube_client::{QubeClient, SelectK8sResourceBy};
use aws_types::SdkConfig;
use chrono::Duration as ChronoDuration;
use jsonptr::Pointer;
use k8s_openapi::api::core::v1::Node;
use retry::OperationResult;
use retry::delay::Fixed;
use std::str::FromStr;
use std::string::ToString;
use std::time::Duration;

const KARPENTER_NAMESPACE: &str = "kube-system";
const KARPENTER_LABEL_SELECTOR: &str = "app.kubernetes.io/instance=karpenter";
const KARPENTER_EXPECTED_POD_COUNT: u32 = 2;
const KARPENTER_DEPLOYMENT_NAME: &str = "karpenter";
const KARPENTER_MIN_NODES_DRAIN_TIMEOUT: ChronoDuration = ChronoDuration::seconds(60);

pub struct Karpenter {}

impl Karpenter {
    pub async fn pause(
        kubernetes: &EKS,
        cloud_provider: &dyn CloudProvider,
        client: &QubeClient,
    ) -> Result<(), Box<EngineError>> {
        let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));

        Self::delete_nodes_spawned_by_karpenter(kubernetes, cloud_provider, client, &event_details).await?;

        // scale down the karpenter deployment
        client
            .set_deployment_replicas_number(
                event_details,
                KARPENTER_DEPLOYMENT_NAME,
                &HelmChartNamespaces::KubeSystem.to_string(),
                0,
            )
            .await
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

        Self::delete_nodes_spawned_by_karpenter(kubernetes, cloud_provider, client, &event_details).await?;

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

    async fn delete_nodes_spawned_by_karpenter(
        kubernetes: &EKS,
        cloud_provider: &dyn CloudProvider,
        client: &QubeClient,
        event_details: &EventDetails,
    ) -> Result<(), Box<EngineError>> {
        let karpenter_parameters = kubernetes.get_karpenter_parameters().ok_or_else(|| {
            Box::new(EngineError::new_k8s_delete_karpenter_nodes_error(
                event_details.clone(),
                CommandError::new_from_safe_message("Karpenter parameters are missing".to_string()),
            ))
        })?;

        let nodes = Self::get_nodes_spawned_by_karpenter(client, event_details).await?;
        if nodes.is_empty() {
            return Ok(());
        }

        let max_nodes_drain_in_sec = karpenter_parameters
            .max_node_drain_time_in_secs
            .map(|duration| ChronoDuration::seconds(duration as i64));
        let nodes_drain_timeout = get_nodes_drain_timeout(client, event_details, max_nodes_drain_in_sec).await?;

        // Uninstall karpenter-configuration chart then Karpenter will delete the nodes
        // The Ec2nodeclasses has a finalizer that wait for the NodeClaims to be terminated
        // The NodeClaims has a finalizer that wait for the Nodes to be terminated
        if let Err(e) = uninstall_chart(
            kubernetes,
            cloud_provider,
            event_details,
            &KarpenterConfigurationChart::chart_name(),
            &HelmChartNamespaces::KubeSystem.to_string(),
            Some(nodes_drain_timeout),
        ) {
            // this error is not blocking because it will be the case if some PDB prevent the nodes to be stopped
            kubernetes
                .logger()
                .log(EngineEvent::Warning(event_details.clone(), EventMessage::from(*e)));
        }

        // remove finalizer of the remaining nodes
        let nodes = client
            .get_nodes(
                event_details.clone(),
                SelectK8sResourceBy::LabelsSelector("karpenter.sh/nodepool".to_string()),
            )
            .await?;

        let patch_operations = vec![json_patch::PatchOperation::Remove(json_patch::RemoveOperation {
            path: Pointer::from_static("/metadata/finalizers").to_buf(),
        })];

        for node in nodes {
            match client.patch_node(event_details.clone(), node, &patch_operations).await {
                Ok(_) => {}
                Err(error) => warn!(
                    "Error while removing node finalizers: {}",
                    error.message(ErrorMessageVerbosity::FullDetails)
                ),
            }
        }

        // wait for Ec2NodeClasses to be deleted
        let mut nb_retry = 0;
        let ec2_node_classes = loop {
            let result = client.get_ec2_node_classes(event_details).await;
            if nb_retry > 20 {
                break result;
            } else {
                match result {
                    Ok(items) if items.is_empty() => break Ok(items),
                    Ok(items) => {
                        info!("nb of EC2NodeClass {}", items.len());
                    }
                    Err(e) => {
                        warn!("Error when trying to get EC2NodeClass {}", e)
                    }
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
                nb_retry += 1;
            }
        }?;

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
            kubernetes.advanced_settings().pleco_resources_ttl,
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

async fn get_nodes_drain_timeout(
    kube_client: &QubeClient,
    event_details: &EventDetails,
    max_nodes_drain_duration: Option<ChronoDuration>,
) -> Result<ChronoDuration, Box<EngineError>> {
    let pods_list = kube_client
        .get_pods(event_details.clone(), None, SelectK8sResourceBy::All)
        .await
        .unwrap_or_else(|_| Vec::with_capacity(0));

    let max_termination_grace_period_seconds = pods_list
        .iter()
        .map(|pod| {
            pod.metadata
                .termination_grace_period_seconds
                .unwrap_or(ChronoDuration::seconds(0))
        })
        .max();
    let timeout = match max_termination_grace_period_seconds {
        None => KARPENTER_MIN_NODES_DRAIN_TIMEOUT,
        Some(duration) => ChronoDuration::max(duration, KARPENTER_MIN_NODES_DRAIN_TIMEOUT),
    };

    match max_nodes_drain_duration {
        None => Ok(timeout),
        Some(max_duration) => Ok(ChronoDuration::min(timeout, max_duration)),
    }
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
