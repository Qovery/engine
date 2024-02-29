use crate::cloud_provider::aws::kubernetes::eks_helm_charts::get_qovery_terraform_config;
use crate::cloud_provider::aws::kubernetes::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use crate::cloud_provider::aws::regions::AwsRegion;
use crate::cloud_provider::helm::{ChartInfo, HelmChartNamespaces};
use crate::cloud_provider::helm_charts::ToCommonHelmChart;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::CloudProvider;
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage};
use crate::models::ToCloudProviderFormat;
use crate::services::kube_client::QubeClient;
use std::str::FromStr;
use std::string::ToString;
use std::time::Duration;

const KARPENTER_DEPLOYMENT_NAME: &str = "karpenter";
const KARPENTER_DEFAULT_NODES_DRAIN_TIMEOUT_IN_SEC: i32 = 60;

pub struct Karpenter {}

impl Karpenter {
    pub async fn pause(
        kubernetes: &dyn Kubernetes,
        cloud_provider: &dyn CloudProvider,
        client: QubeClient,
        nodes_drain_timeout_in_sec: Option<i32>,
    ) -> Result<(), Box<EngineError>> {
        let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));

        Self::delete_nodes_spawned_by_karpenter(
            kubernetes,
            cloud_provider,
            &client,
            event_details.clone(),
            nodes_drain_timeout_in_sec,
        )
        .await?;

        // wait for Ec2nodeclasses to be deleted
        // TODO PG: find how to use kube client to list CRD
        tokio::time::sleep(Duration::from_secs(30)).await;

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
        kubernetes: &dyn Kubernetes,
        cloud_provider: &dyn CloudProvider,
        client: QubeClient,
        kubernetes_long_id: uuid::Uuid,
        disk_size_in_gib: Option<i32>,
        qovery_terraform_config_file: &str,
    ) -> Result<(), Box<EngineError>> {
        let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));

        // scale up the karpenter deployment
        client
            .set_deployment_replicas_number(
                event_details.clone(),
                KARPENTER_DEPLOYMENT_NAME,
                &HelmChartNamespaces::KubeSystem.to_string(),
                2,
            )
            .await?;

        Self::install_karpenter_configuration(
            kubernetes,
            cloud_provider,
            event_details,
            kubernetes_long_id,
            disk_size_in_gib,
            qovery_terraform_config_file,
        )
    }

    async fn delete_nodes_spawned_by_karpenter(
        kubernetes: &dyn Kubernetes,
        cloud_provider: &dyn CloudProvider,
        client: &QubeClient,
        event_details: EventDetails,
        nodes_drain_timeout_in_sec: Option<i32>,
    ) -> Result<(), Box<EngineError>> {
        let kubernetes_config_file_path = kubernetes.kubeconfig_local_file_path();

        // 1 uninstall karpenter-configuration chart
        let helm = Helm::new(
            &kubernetes_config_file_path,
            &cloud_provider.credentials_environment_variables(),
        )
        .map_err(|e| to_engine_error(&event_details, e))?;

        let mut chart = ChartInfo::new_from_release_name(
            &KarpenterConfigurationChart::chart_name(),
            &HelmChartNamespaces::KubeSystem.to_string(),
        );
        chart.timeout_in_seconds =
            nodes_drain_timeout_in_sec.unwrap_or(KARPENTER_DEFAULT_NODES_DRAIN_TIMEOUT_IN_SEC) as i64;

        if let Err(e) = helm.uninstall(&chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
            // this error is not blocking because it will be the case if some PDB prevent the nodes to be stopped
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new_from_engine_error(to_engine_error(&event_details, e)),
            ));
        }

        // 2 remove finalizer of the remaining nodes
        let nodes = client
            .get_nodes(
                event_details.clone(),
                crate::services::kube_client::SelectK8sResourceBy::LabelsSelector("karpenter.sh/nodepool".to_string()),
            )
            .await?;

        let patch_operations = vec![json_patch::PatchOperation::Remove(json_patch::RemoveOperation {
            path: "/metadata/finalizers".to_string(),
        })];

        for node in nodes {
            client
                .patch_node(event_details.clone(), node, &patch_operations)
                .await?;
        }
        Ok(())
    }

    fn install_karpenter_configuration(
        kubernetes: &dyn Kubernetes,
        cloud_provider: &dyn CloudProvider,
        event_details: EventDetails,
        cluster_long_id: uuid::Uuid,
        disk_size_in_gib: Option<i32>,
        qovery_terraform_config_file: &str,
    ) -> Result<(), Box<EngineError>> {
        let kubernetes_config_file_path = kubernetes.kubeconfig_local_file_path();
        let helm = Helm::new(kubernetes_config_file_path, &cloud_provider.credentials_environment_variables())
            .map_err(|e| to_engine_error(&event_details, e))?;

        let karpenter_configuration_chart = Self::get_karpenter_configuration_chart(
            kubernetes,
            cloud_provider,
            cluster_long_id,
            disk_size_in_gib,
            qovery_terraform_config_file,
        )?;

        Ok(helm
            .upgrade(&karpenter_configuration_chart, &[], &CommandKiller::never())
            .map_err(|e| {
                EngineError::new_helm_charts_upgrade_error(
                    event_details.clone(),
                    CommandError::new(
                        "can't helm upgrade karpenter-configuration".to_string(),
                        Some(e.to_string()),
                        None,
                    ),
                )
            })?)
    }

    fn get_karpenter_configuration_chart(
        kubernetes: &dyn Kubernetes,
        cloud_provider: &dyn CloudProvider,
        cluster_long_id: uuid::Uuid,
        disk_size_in_gib: Option<i32>,
        qovery_terraform_config_file: &str,
    ) -> Result<ChartInfo, Box<EngineError>> {
        let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));

        let qovery_terraform_config = get_qovery_terraform_config(qovery_terraform_config_file, &[]).map_err(|e| {
            EngineError::new_k8s_node_not_ready(
                event_details.clone(),
                CommandError::new_from_safe_message(format!("Cannot get qovery_terraform_config: {e}")),
            )
        })?;

        let organization_id = cloud_provider.organization_id().to_string();
        let organization_long_id = cloud_provider.organization_long_id();
        let cluster_id = kubernetes.id().to_string();
        let region = AwsRegion::from_str(kubernetes.region()).map_err(|_e| {
            EngineError::new_unsupported_region(event_details.clone(), kubernetes.region().to_string(), None)
        })?;
        let cluster_name = kubernetes.cluster_name();

        // Karpenter Configuration
        let karpenter_configuration_chart = KarpenterConfigurationChart::new(
            Some(kubernetes.temp_dir().to_string_lossy().as_ref()),
            cluster_name.to_string(),
            true,
            qovery_terraform_config.cluster_security_group_id,
            disk_size_in_gib,
            &cluster_id,
            cluster_long_id,
            &organization_id,
            organization_long_id,
            region.to_cloud_provider_format(),
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

        Ok(karpenter_configuration_chart.chart_info)
    }
}
