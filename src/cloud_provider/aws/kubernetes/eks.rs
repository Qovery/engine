use crate::cloud_provider::aws::kubernetes;
use crate::cloud_provider::aws::kubernetes::node::AwsInstancesType;
use crate::cloud_provider::aws::kubernetes::{KarpenterParameters, Options};
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZone};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::{
    event_details, send_progress_on_long_task, InstanceType, Kind, Kubernetes, KubernetesNodesType,
    KubernetesUpgradeStatus, KubernetesVersion,
};
use crate::cloud_provider::models::CpuArchitecture;
use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::CloudProvider;
use crate::cmd::kubectl::{kubectl_exec_scale_replicas, ScalingKind};
use crate::cmd::terraform::terraform_init_validate_plan_apply;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep};
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::logger::Logger;
use crate::runtime::block_on;
use crate::secret_manager::vault::QVaultClient;
use crate::services::aws::models::QoveryAwsSdkConfigEks;
use crate::services::kube_client::SelectK8sResourceBy;
use async_trait::async_trait;
use aws_types::SdkConfig;

use aws_sdk_eks::error::SdkError;

use crate::cloud_provider::aws::kubernetes::ec2::mk_s3;
use crate::cloud_provider::kubeconfig_helper::{fetch_kubeconfig, write_kubeconfig_on_disk};
use crate::cloud_provider::kubectl_utils::{check_workers_on_upgrade, delete_completed_jobs, delete_crashlooping_pods};
use crate::engine::InfrastructureContext;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::s3::S3;
use aws_sdk_eks::operation::delete_nodegroup::{DeleteNodegroupError, DeleteNodegroupOutput};
use aws_sdk_eks::operation::describe_cluster::{DescribeClusterError, DescribeClusterOutput};
use aws_sdk_eks::operation::describe_nodegroup::{DescribeNodegroupError, DescribeNodegroupOutput};
use aws_sdk_eks::operation::list_clusters::{ListClustersError, ListClustersOutput};
use aws_sdk_eks::operation::list_nodegroups::{ListNodegroupsError, ListNodegroupsOutput};
use aws_sdk_iam::operation::create_service_linked_role::{CreateServiceLinkedRoleError, CreateServiceLinkedRoleOutput};
use aws_sdk_iam::operation::get_role::{GetRoleError, GetRoleOutput};
use base64::engine::general_purpose;
use base64::Engine;
use function_name::named;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::cmd::terraform_validators::TerraformValidators;
use thiserror::Error;
use uuid::Uuid;

use super::{
    define_cluster_upgrade_timeout, get_rusoto_eks_client, should_update_desired_nodes,
    AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION,
};

/// EKS kubernetes provider allowing to deploy an EKS cluster.
pub struct EKS {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    version: KubernetesVersion,
    region: AwsRegion,
    zones: Vec<AwsZone>,
    s3: S3,
    nodes_groups: Vec<NodeGroups>,
    template_directory: String,
    options: Options,
    logger: Box<dyn Logger>,
    advanced_settings: ClusterAdvancedSettings,
    customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    kubeconfig: Option<String>,
    temp_dir: PathBuf,
    qovery_allowed_public_access_cidrs: Option<Vec<String>>,
}

impl EKS {
    pub fn new(
        context: Context,
        id: &str,
        long_id: Uuid,
        name: &str,
        version: KubernetesVersion,
        region: AwsRegion,
        zones: Vec<String>,
        cloud_provider: &dyn CloudProvider,
        options: Options,
        nodes_groups: Vec<NodeGroups>,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
        kubeconfig: Option<String>,
        temp_dir: PathBuf,
        qovery_allowed_public_access_cidrs: Option<Vec<String>>,
    ) -> Result<Self, Box<EngineError>> {
        let event_details = event_details(cloud_provider, long_id, name.to_string(), &context);
        let template_directory = format!("{}/aws/bootstrap", context.lib_root_dir());

        let aws_zones = kubernetes::aws_zones(zones, &region, &event_details)?;

        // ensure config is ok
        if let Err(e) = EKS::validate_node_groups(nodes_groups.clone(), &event_details) {
            logger.log(EngineEvent::Error(*e.clone(), None));
            return Err(Box::new(*e));
        };
        advanced_settings.validate(event_details.clone())?;

        let s3 = mk_s3(&region, cloud_provider);

        let cluster = EKS {
            context,
            id: id.to_string(),
            long_id,
            name: name.to_string(),
            version,
            region,
            zones: aws_zones,
            s3,
            options,
            nodes_groups,
            template_directory,
            logger,
            advanced_settings,
            customer_helm_charts_override,
            kubeconfig,
            temp_dir,
            qovery_allowed_public_access_cidrs,
        };

        if let Some(kubeconfig) = &cluster.kubeconfig {
            write_kubeconfig_on_disk(
                &cluster.kubeconfig_local_file_path(),
                kubeconfig,
                cluster.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
            )?;
        } else {
            fetch_kubeconfig(&cluster, &cluster.s3)?;
        }

        Ok(cluster)
    }

    pub fn validate_node_groups(
        nodes_groups: Vec<NodeGroups>,
        event_details: &EventDetails,
    ) -> Result<(), Box<EngineError>> {
        for node_group in &nodes_groups {
            match AwsInstancesType::from_str(node_group.instance_type.as_str()) {
                Ok(x) => {
                    if !EKS::is_instance_allowed(x) {
                        let err = EngineError::new_not_allowed_instance_type(
                            event_details.clone(),
                            node_group.instance_type.as_str(),
                        );
                        return Err(Box::new(err));
                    }
                }
                Err(e) => {
                    let err = EngineError::new_unsupported_instance_type(
                        event_details.clone(),
                        node_group.instance_type.as_str(),
                        e,
                    );
                    return Err(Box::new(err));
                }
            }
        }
        Ok(())
    }

    pub fn is_instance_allowed(instance_type: AwsInstancesType) -> bool {
        instance_type.is_instance_cluster_allowed()
    }

    fn set_cluster_autoscaler_replicas(
        &self,
        event_details: EventDetails,
        replicas_count: u32,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>> {
        let autoscaler_new_state = match replicas_count {
            0 => "disable",
            _ => "enable",
        };
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Set cluster autoscaler to: `{autoscaler_new_state}`.")),
        ));
        let selector = "cluster-autoscaler-aws-cluster-autoscaler";
        let namespace = "kube-system";
        kubectl_exec_scale_replicas(
            self.kubeconfig_local_file_path(),
            infra_ctx.cloud_provider().credentials_environment_variables(),
            namespace,
            ScalingKind::Deployment,
            selector,
            replicas_count,
        )
        .map_err(|e| {
            Box::new(EngineError::new_k8s_scale_replicas(
                event_details.clone(),
                selector.to_string(),
                namespace.to_string(),
                replicas_count,
                e,
            ))
        })?;

        Ok(())
    }

    fn struct_name(&self) -> &str {
        "kubernetes"
    }
}

impl Kubernetes for EKS {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Eks
    }

    fn as_kubernetes(&self) -> &dyn Kubernetes {
        self
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> KubernetesVersion {
        self.version.clone()
    }

    fn region(&self) -> &str {
        self.region.to_cloud_provider_format()
    }

    fn zones(&self) -> Option<Vec<&str>> {
        Some(self.zones.iter().map(|z| z.to_cloud_provider_format()).collect())
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn is_network_managed_by_user(&self) -> bool {
        self.options.user_provided_network.is_some()
    }

    fn is_self_managed(&self) -> bool {
        false
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        if let Some(karpenter_parameters) = &self.options.karpenter_parameters {
            vec![karpenter_parameters.default_service_architecture]
        } else {
            self.nodes_groups.iter().map(|x| x.instance_architecture).collect()
        }
    }

    #[named]
    fn on_create(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            infra_ctx.cloud_provider().name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || {
            kubernetes::create(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                &self.s3,
                self.long_id,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
                &self.advanced_settings,
                self.qovery_allowed_public_access_cidrs.as_ref(),
            )
        })
    }

    fn upgrade_with_status(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Start preparing EKS cluster upgrade process".to_string()),
        ));

        let temp_dir = self.temp_dir();
        let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), self, infra_ctx.cloud_provider()) {
            Ok(value) => Some(value),
            Err(_) => None,
        };

        let node_groups_with_desired_states = should_update_desired_nodes(
            event_details.clone(),
            self,
            KubernetesClusterAction::Upgrade(None),
            &self.nodes_groups,
            aws_eks_client,
        )?;

        // in case error, this should no be in the blocking process
        let mut cluster_upgrade_timeout_in_min = *AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
        if let Ok(kube_client) = infra_ctx.mk_kube_client() {
            let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
                .unwrap_or_else(|_| Vec::with_capacity(0));

            let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Upgrade(None));
            cluster_upgrade_timeout_in_min = timeout;

            if let Some(x) = message {
                self.logger()
                    .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
            }
        };

        // generate terraform files and copy them into temp dir
        let mut context = kubernetes::tera_context(
            self,
            infra_ctx.cloud_provider(),
            infra_ctx.dns_provider(),
            &self.zones,
            &node_groups_with_desired_states,
            &self.options,
            cluster_upgrade_timeout_in_min,
            false,
            &self.advanced_settings,
            self.qovery_allowed_public_access_cidrs.as_ref(),
        )?;

        //
        // Upgrade master nodes
        //
        match &kubernetes_upgrade_status.required_upgrade_on {
            Some(KubernetesNodesType::Masters) => {
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Start upgrading process for master nodes.".to_string()),
                ));

                // AWS requires the upgrade to be done in 2 steps (masters, then workers)
                // use the current kubernetes masters' version for workers, in order to avoid migration in one step
                context.insert(
                    "kubernetes_master_version",
                    format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
                );
                // use the current master version for workers, they will be updated later
                context.insert(
                    "eks_workers_version",
                    format!("{}", &kubernetes_upgrade_status.deployed_masters_version).as_str(),
                );

                if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
                    self.template_directory.as_str(),
                    temp_dir,
                    context.clone(),
                ) {
                    return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                        event_details,
                        self.template_directory.to_string(),
                        temp_dir.to_string_lossy().to_string(),
                        e,
                    )));
                }

                let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
                let common_bootstrap_charts = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
                if let Err(e) = crate::template::copy_non_template_files(
                    common_bootstrap_charts.as_str(),
                    common_charts_temp_dir.as_str(),
                ) {
                    return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                        event_details,
                        common_bootstrap_charts,
                        common_charts_temp_dir,
                        e,
                    )));
                }

                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Upgrading Kubernetes master nodes.".to_string()),
                ));

                match terraform_init_validate_plan_apply(
                    temp_dir.to_string_lossy().as_ref(),
                    self.context.is_dry_run_deploy(),
                    infra_ctx
                        .cloud_provider()
                        .credentials_environment_variables()
                        .as_slice(),
                    &TerraformValidators::Default,
                ) {
                    Ok(_) => {
                        self.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(
                                "Kubernetes master nodes have been successfully upgraded.".to_string(),
                            ),
                        ));
                    }
                    Err(e) => {
                        return Err(Box::new(EngineError::new_terraform_error(event_details, e)));
                    }
                }
            }
            Some(KubernetesNodesType::Workers) => {
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(
                        "No need to perform Kubernetes master upgrade, they are already up to date.".to_string(),
                    ),
                ));
            }
            None => {
                self.logger().log(EngineEvent::Info(
                    event_details,
                    EventMessage::new_from_safe(
                        "No Kubernetes upgrade required, masters and workers are already up to date.".to_string(),
                    ),
                ));
                return Ok(());
            }
        }

        //
        // Upgrade worker nodes
        //
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing workers nodes for upgrade for Kubernetes cluster.".to_string()),
        ));

        // disable cluster autoscaler to avoid interfering with AWS upgrade procedure
        context.insert("enable_cluster_autoscaler", &false);
        context.insert(
            "eks_workers_version",
            format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
        );

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir,
            context.clone(),
        ) {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir.to_string_lossy().to_string(),
                e,
            )));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
        let common_bootstrap_charts = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
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

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Starting Kubernetes worker nodes upgrade".to_string()),
        ));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Checking clusters content health".to_string()),
        ));

        // disable all replicas with issues to avoid upgrade failures
        let kube_client = infra_ctx.mk_kube_client()?;
        let deployments = block_on(kube_client.get_deployments(event_details.clone(), None, SelectK8sResourceBy::All))?;
        for deploy in deployments {
            let status = match deploy.status {
                Some(s) => s,
                None => continue,
            };

            let replicas = status.replicas.unwrap_or(0);
            let ready_replicas = status.ready_replicas.unwrap_or(0);

            // if number of replicas > 0: it is not already disabled
            // ready_replicas == 0: there is something in progress (rolling restart...) so we should not touch it
            if replicas > 0 && ready_replicas == 0 {
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!(
                        "Deployment {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                        deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
                    )),
                ));
                block_on(kube_client.set_deployment_replicas_number(
                    event_details.clone(),
                    deploy.metadata.name.as_str(),
                    deploy.metadata.namespace.as_str(),
                    0,
                ))?;
            } else {
                info!(
                    "Deployment {}/{} has {}/{} replicas ready. No action needed.",
                    deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
                );
            }
        }
        // same with statefulsets
        let statefulsets =
            block_on(kube_client.get_statefulsets(event_details.clone(), None, SelectK8sResourceBy::All))?;
        for sts in statefulsets {
            let status = match sts.status {
                Some(s) => s,
                None => continue,
            };

            let ready_replicas = status.ready_replicas.unwrap_or(0);

            // if number of replicas > 0: it is not already disabled
            // ready_replicas == 0: there is something in progress (rolling restart...) so we should not touch it
            if status.replicas > 0 && ready_replicas == 0 {
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!(
                        "Statefulset {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                        sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
                    )),
                ));
                block_on(kube_client.set_statefulset_replicas_number(
                    event_details.clone(),
                    sts.metadata.name.as_str(),
                    sts.metadata.namespace.as_str(),
                    0,
                ))?;
            } else {
                info!(
                    "Statefulset {}/{} has {}/{} replicas ready. No action needed.",
                    sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
                );
            }
        }

        if let Err(e) = delete_crashlooping_pods(
            self,
            None,
            None,
            Some(3),
            infra_ctx.cloud_provider().credentials_environment_variables(),
            Infrastructure(InfrastructureStep::Upgrade),
        ) {
            self.logger().log(EngineEvent::Error(*e.clone(), None));
            return Err(e);
        }

        if let Err(e) = delete_completed_jobs(
            self,
            infra_ctx.cloud_provider().credentials_environment_variables(),
            Infrastructure(InfrastructureStep::Upgrade),
            None,
        ) {
            self.logger().log(EngineEvent::Error(*e.clone(), None));
            return Err(e);
        }

        if !infra_ctx.kubernetes().is_karpenter_enabled() {
            // Disable cluster autoscaler deployment and be sure we re-enable it on exist
            let ev = event_details.clone();
            let _guard = scopeguard::guard(
                self.set_cluster_autoscaler_replicas(event_details.clone(), 0, infra_ctx)?,
                |_| {
                    let _ = self.set_cluster_autoscaler_replicas(ev, 1, infra_ctx);
                },
            );
        }

        terraform_init_validate_plan_apply(
            temp_dir.to_string_lossy().as_ref(),
            self.context.is_dry_run_deploy(),
            infra_ctx
                .cloud_provider()
                .credentials_environment_variables()
                .as_slice(),
            &TerraformValidators::Default,
        )
        .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

        check_workers_on_upgrade(
            self,
            infra_ctx.cloud_provider(),
            kubernetes_upgrade_status.requested_version.to_string(),
            match self.is_karpenter_enabled() {
                true => Some("eks.amazonaws.com/compute-type!=fargate"),
                false => None,
            },
        )
        .map_err(|e| EngineError::new_k8s_node_not_ready(event_details.clone(), e))?;

        self.logger().log(EngineEvent::Info(
            event_details,
            EventMessage::new_from_safe("Kubernetes nodes have been successfully upgraded".to_string()),
        ));

        Ok(())
    }

    #[named]
    fn on_pause(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            infra_ctx.cloud_provider().name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || {
            kubernetes::pause(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
                &self.advanced_settings,
                self.qovery_allowed_public_access_cidrs.as_ref(),
            )
        })
    }

    #[named]
    fn on_delete(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            infra_ctx.cloud_provider().name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || {
            kubernetes::delete(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                &self.s3,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
                &self.advanced_settings,
                self.qovery_allowed_public_access_cidrs.as_ref(),
            )
        })
    }

    fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    fn update_vault_config(
        &self,
        event_details: EventDetails,
        cluster_secrets: crate::cloud_provider::vault::ClusterSecrets,
        kubeconfig_file_path: Option<&Path>,
    ) -> Result<(), Box<EngineError>> {
        let vault_conn = match QVaultClient::new(event_details.clone()) {
            Ok(x) => Some(x),
            Err(_) => None,
        };
        if let Some(vault) = vault_conn {
            // encode base64 kubeconfig
            let kubeconfig = match kubeconfig_file_path {
                Some(x) => fs::read_to_string(x)
                    .map_err(|e| {
                        EngineError::new_cannot_retrieve_cluster_config_file(
                            event_details.clone(),
                            CommandError::new_from_safe_message(format!(
                                "Cannot read kubeconfig file {}: {e}",
                                x.to_str().unwrap_or_default()
                            )),
                        )
                    })
                    .expect("kubeconfig was not found while it should be present"),
                None => "".to_string(),
            };
            let kubeconfig_b64 = general_purpose::STANDARD.encode(kubeconfig);

            let mut cluster_secrets_update = cluster_secrets;
            cluster_secrets_update.set_kubeconfig_b64(kubeconfig_b64);

            // update info without taking care of the kubeconfig because we don't have it yet
            let _ = cluster_secrets_update.create_or_update_secret(&vault, false, event_details.clone());
        };

        Ok(())
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn customer_helm_charts_override(&self) -> Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>> {
        self.customer_helm_charts_override.clone()
    }

    fn is_karpenter_enabled(&self) -> bool {
        self.options.karpenter_parameters.is_some() || self.advanced_settings.aws_enable_karpenter
    }

    fn get_karpenter_parameters(&self) -> Option<KarpenterParameters> {
        if let Some(karpenter_parameters) = &self.options.karpenter_parameters {
            return Some(KarpenterParameters {
                spot_enabled: karpenter_parameters.spot_enabled,
                max_node_drain_time_in_secs: karpenter_parameters.max_node_drain_time_in_secs,
                disk_size_in_gib: karpenter_parameters.disk_size_in_gib,
                default_service_architecture: karpenter_parameters.default_service_architecture,
            });
        }

        if self.advanced_settings.aws_enable_karpenter {
            if let Some(node_group) = self.nodes_groups.first() {
                return Some(KarpenterParameters {
                    spot_enabled: self.advanced_settings.aws_karpenter_enable_spot,
                    max_node_drain_time_in_secs: self.advanced_settings.aws_karpenter_max_node_drain_in_sec,
                    disk_size_in_gib: node_group.disk_size_in_gib,
                    default_service_architecture: node_group.instance_architecture,
                });
            }
        }

        None
    }

    fn loadbalancer_l4_annotations(&self, cloud_provider_lb_name: Option<&str>) -> Vec<(String, String)> {
        let lb_name = match cloud_provider_lb_name {
            Some(x) => format!(",QoveryName={x}"),
            None => "".to_string(),
        };
        match self.advanced_settings().aws_eks_enable_alb_controller {
            // !!! IMPORTANT !!!
            // Changing this may require destroy/recreate a load balancer (and so downtime)
            true => {
                vec![
                    (
                        "service.beta.kubernetes.io/aws-load-balancer-type".to_string(),
                        "external".to_string(),
                    ),
                    (
                        "service.beta.kubernetes.io/aws-load-balancer-scheme".to_string(),
                        "internet-facing".to_string(),
                    ),
                    (
                        "service.beta.kubernetes.io/aws-load-balancer-nlb-target-type".to_string(),
                        "ip".to_string(),
                    ),
                    (
                        "service.beta.kubernetes.io/aws-load-balancer-additional-resource-tags".to_string(),
                        format!(
                            "OrganizationLongId={},OrganizationId={},ClusterLongId={},ClusterId={}{}",
                            self.context.organization_long_id(),
                            self.context.organization_short_id(),
                            self.as_kubernetes().long_id(),
                            self.as_kubernetes().id(),
                            lb_name
                        ),
                    ),
                ]
            }
            false => vec![(
                "service.beta.kubernetes.io/aws-load-balancer-type".to_string(),
                "nlb".to_string(),
            )],
        }
    }
}

#[cfg(test)]
impl NodeGroupsWithDesiredState {
    fn new(
        name: String,
        id: Option<String>,
        min_nodes: i32,
        max_nodes: i32,
        desired_size: i32,
        enable_desired_size: bool,
        instance_type: String,
        disk_size_in_gib: i32,
    ) -> NodeGroupsWithDesiredState {
        NodeGroupsWithDesiredState {
            name,
            id,
            min_nodes,
            max_nodes,
            desired_size,
            enable_desired_size,
            instance_type,
            disk_size_in_gib,
            instance_architecture: CpuArchitecture::AMD64,
        }
    }
}

pub fn select_nodegroups_autoscaling_group_behavior(
    action: KubernetesClusterAction,
    nodegroup: &NodeGroups,
) -> NodeGroupsWithDesiredState {
    let nodegroup_desired_state = |x| {
        // desired nodes can't be lower than min nodes
        if x < nodegroup.min_nodes {
            (true, nodegroup.min_nodes)
            // desired nodes can't be higher than max nodes
        } else if x > nodegroup.max_nodes {
            (true, nodegroup.max_nodes)
        } else {
            (false, x)
        }
    };

    match action {
        KubernetesClusterAction::Bootstrap => {
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, nodegroup.min_nodes, true)
        }
        KubernetesClusterAction::Update(current_nodes) | KubernetesClusterAction::Upgrade(current_nodes) => {
            let (upgrade_required, desired_state) = match current_nodes {
                Some(x) => nodegroup_desired_state(x),
                // if nothing is given, it's may be because the nodegroup has been deleted manually, so if we need to set it otherwise we won't be able to create a new nodegroup
                None => (true, nodegroup.max_nodes),
            };
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, desired_state, upgrade_required)
        }
        KubernetesClusterAction::Pause | KubernetesClusterAction::Delete => {
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, nodegroup.min_nodes, false)
        }
        KubernetesClusterAction::Resume(current_nodes) => {
            // we always want to set the desired sate here to optimize the speed to return to the best situation
            // TODO: (pmavro) save state on pause and reread it on resume
            let resume_nodes_number = match current_nodes {
                Some(x) => nodegroup_desired_state(x).1,
                None => nodegroup.min_nodes,
            };
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, resume_nodes_number, true)
        }
        KubernetesClusterAction::CleanKarpenterMigration => {
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, 0, false)
        }
    }
}

#[async_trait]
impl QoveryAwsSdkConfigEks for SdkConfig {
    async fn list_clusters(&self) -> Result<ListClustersOutput, SdkError<ListClustersError>> {
        let client = aws_sdk_eks::Client::new(self);
        client.list_clusters().send().await
    }

    async fn describe_cluster(
        &self,
        cluster_id: String,
    ) -> Result<DescribeClusterOutput, SdkError<DescribeClusterError>> {
        let client = aws_sdk_eks::Client::new(self);
        client.describe_cluster().name(cluster_id).send().await
    }

    async fn list_all_eks_nodegroups(
        &self,
        cluster_name: String,
    ) -> Result<ListNodegroupsOutput, SdkError<ListNodegroupsError>> {
        let client = aws_sdk_eks::Client::new(self);
        client.list_nodegroups().cluster_name(cluster_name).send().await
    }

    async fn describe_nodegroup(
        &self,
        cluster_name: String,
        nodegroup_id: String,
    ) -> Result<DescribeNodegroupOutput, SdkError<DescribeNodegroupError>> {
        let client = aws_sdk_eks::Client::new(self);
        client
            .describe_nodegroup()
            .cluster_name(cluster_name)
            .nodegroup_name(nodegroup_id)
            .send()
            .await
    }

    async fn describe_nodegroups(
        &self,
        cluster_name: String,
        nodegroups: ListNodegroupsOutput,
    ) -> Result<Vec<DescribeNodegroupOutput>, SdkError<DescribeNodegroupError>> {
        let mut nodegroups_descriptions = Vec::new();

        for nodegroup in nodegroups.nodegroups.unwrap_or_default() {
            let nodegroup_description = self.describe_nodegroup(cluster_name.clone(), nodegroup).await;
            match nodegroup_description {
                Ok(x) => nodegroups_descriptions.push(x),
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(nodegroups_descriptions)
    }

    async fn delete_nodegroup(
        &self,
        cluster_name: String,
        nodegroup_name: String,
    ) -> Result<DeleteNodegroupOutput, SdkError<DeleteNodegroupError>> {
        let client = aws_sdk_eks::Client::new(self);
        client
            .delete_nodegroup()
            .cluster_name(cluster_name)
            .nodegroup_name(nodegroup_name)
            .send()
            .await
    }

    async fn get_role(&self, name: &str) -> Result<GetRoleOutput, SdkError<GetRoleError>> {
        let client = aws_sdk_iam::Client::new(self);
        client.get_role().role_name(name).send().await
    }

    async fn create_service_linked_role(
        &self,
        service_name: &str,
    ) -> Result<CreateServiceLinkedRoleOutput, SdkError<CreateServiceLinkedRoleError>> {
        let client = aws_sdk_iam::Client::new(self);
        client
            .create_service_linked_role()
            .aws_service_name(service_name)
            .send()
            .await
    }
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum NodeGroupToRemoveFailure {
    #[error("No cluster found")]
    ClusterNotFound,
    #[error("No nodegroup found for this cluster")]
    NodeGroupNotFound,
    #[error("At lease one nodegroup must be active, no one can be deleted")]
    OneNodeGroupMustBeActiveAtLeast,
}

#[derive(PartialEq, Eq)]
pub enum NodeGroupsDeletionType {
    All,
    FailedOnly,
}

pub async fn delete_eks_nodegroups(
    aws_conn: SdkConfig,
    cluster_name: String,
    is_first_install: bool,
    nodegroup_delete_selection: NodeGroupsDeletionType,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    let clusters = match aws_conn.list_clusters().await {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_cannot_list_clusters_error(
                event_details.clone(),
                CommandError::new("Couldn't list clusters from AWS".to_string(), Some(e.to_string()), None),
            )));
        }
    };

    if !clusters.clusters().iter().any(|x| x == &cluster_name) {
        return Err(Box::new(EngineError::new_cannot_get_cluster_error(
            event_details.clone(),
            CommandError::new_from_safe_message(NodeGroupToRemoveFailure::ClusterNotFound.to_string()),
        )));
    };

    let all_cluster_nodegroups = match aws_conn.list_all_eks_nodegroups(cluster_name.clone()).await {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_nodegroup_list_error(
                event_details,
                CommandError::new_from_safe_message(e.to_string()),
            )));
        }
    };

    let all_cluster_nodegroups_described = match aws_conn
        .describe_nodegroups(cluster_name.clone(), all_cluster_nodegroups)
        .await
    {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                event_details,
                e.to_string(),
            )));
        }
    };

    // If it is the first installation of the cluster, we dont want to keep any nodegroup.
    // So just delete everything
    let nodegroups_to_delete = if is_first_install || nodegroup_delete_selection == NodeGroupsDeletionType::All {
        info!("Deleting all nodegroups of this cluster as it is the first installation.");
        all_cluster_nodegroups_described
    } else {
        match check_failed_nodegroups_to_remove(all_cluster_nodegroups_described.clone()) {
            Ok(x) => x,
            Err(e) => {
                // print AWS nodegroup errors to the customer (useful when quota is reached)
                if e == NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast {
                    let nodegroup_health_message = all_cluster_nodegroups_described
                        .iter()
                        .map(|n| match n.nodegroup() {
                            Some(nodegroup) => {
                                let nodegroup_name = nodegroup.nodegroup_name().unwrap_or("unknown_nodegroup_name");
                                let nodegroup_status = match nodegroup.health() {
                                    Some(x) =>
                                        x
                                            .issues()
                                            .iter()
                                            .map(|x| format!("{:?}: {}", x.code(), x.message().unwrap_or("no AWS specific message given, please contact Qovery and AWS support regarding this nodegroup issue")))
                                            .collect::<Vec<String>>()
                                            .join(", "),
                                    None => "can't get nodegroup status from cloud provider".to_string(),
                                };
                                format!("Nodegroup {nodegroup_name} health is: {nodegroup_status}")
                            }
                            None => "".to_string(),
                        })
                        .collect::<Vec<String>>()
                        .join("\n");

                    return Err(Box::new(EngineError::new_nodegroup_delete_any_nodegroup_error(
                        event_details,
                        nodegroup_health_message,
                    )));
                };

                return Err(Box::new(EngineError::new_nodegroup_delete_error(
                    event_details,
                    None,
                    e.to_string(),
                )));
            }
        }
    };

    for nodegroup in nodegroups_to_delete {
        let nodegroup_name = match nodegroup.nodegroup() {
            Some(x) => x.nodegroup_name().unwrap_or("unknown_nodegroup_name"),
            None => {
                return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                    event_details,
                    format!("{nodegroup:?}"),
                )));
            }
        };

        if let Err(e) = aws_conn
            .delete_nodegroup(cluster_name.clone(), nodegroup_name.to_string())
            .await
        {
            return Err(Box::new(EngineError::new_nodegroup_delete_error(
                event_details,
                Some(nodegroup_name.to_string()),
                e.to_string(),
            )));
        }
    }

    Ok(())
}

fn check_failed_nodegroups_to_remove(
    nodegroups: Vec<DescribeNodegroupOutput>,
) -> Result<Vec<DescribeNodegroupOutput>, NodeGroupToRemoveFailure> {
    let mut failed_nodegroups_to_remove = Vec::new();

    for nodegroup in nodegroups.iter() {
        match nodegroup.nodegroup() {
            Some(ng) => match ng.status() {
                Some(s) => match s {
                    aws_sdk_eks::types::NodegroupStatus::CreateFailed => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    aws_sdk_eks::types::NodegroupStatus::DeleteFailed => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    aws_sdk_eks::types::NodegroupStatus::Degraded => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    _ => {
                        info!(
                            "Nodegroup {} is in state {:?}, it will not be deleted",
                            ng.nodegroup_name().unwrap_or("unknown name"),
                            s
                        );
                        continue;
                    }
                },
                None => continue,
            },
            None => return Err(NodeGroupToRemoveFailure::NodeGroupNotFound),
        }
    }

    // ensure we don't remove all nodegroups (even failed ones) to avoid blackout
    if failed_nodegroups_to_remove.len() == nodegroups.len() && !nodegroups.is_empty() {
        return Err(NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast);
    }

    Ok(failed_nodegroups_to_remove)
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::eks::{
        select_nodegroups_autoscaling_group_behavior, NodeGroupToRemoveFailure, EKS,
    };
    use crate::cloud_provider::models::{
        CpuArchitecture, KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState,
    };
    use crate::errors::Tag;
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;
    use aws_sdk_eks::operation::describe_nodegroup::DescribeNodegroupOutput;
    use aws_sdk_eks::types::{Nodegroup, NodegroupStatus};
    use uuid::Uuid;

    use super::check_failed_nodegroups_to_remove;

    #[test]
    fn test_nodegroup_failure_deletion() {
        let nodegroup_ok = Nodegroup::builder()
            .set_nodegroup_name(Some("nodegroup_ok".to_string()))
            .set_status(Some(NodegroupStatus::Active))
            .build();
        let nodegroup_create_failed = Nodegroup::builder()
            .set_nodegroup_name(Some("nodegroup_create_failed".to_string()))
            .set_status(Some(NodegroupStatus::CreateFailed))
            .build();

        // 2 nodegroups, 2 ok => nothing to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
        ];
        assert_eq!(check_failed_nodegroups_to_remove(ngs).unwrap().len(), 0);

        // 2 nodegroups, 1 ok, 1 create failed => 1 to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
        ];
        let failed_ngs = check_failed_nodegroups_to_remove(ngs).unwrap();
        assert_eq!(failed_ngs.len(), 1);
        assert_eq!(
            failed_ngs[0].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_create_failed"
        );

        // 2 nodegroups, 2 failed => nothing to do, too critical to be deleted. Manual intervention required
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
        ];
        assert_eq!(
            check_failed_nodegroups_to_remove(ngs).unwrap_err(),
            NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast
        );

        // 1 nodegroup, 1 failed => nothing to do, too critical to be deleted. Manual intervention required
        let ngs = vec![DescribeNodegroupOutput::builder()
            .nodegroup(nodegroup_create_failed.clone())
            .build()];
        assert_eq!(
            check_failed_nodegroups_to_remove(ngs).unwrap_err(),
            NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast
        );

        // no nodegroups => ok
        let ngs = vec![];
        assert_eq!(check_failed_nodegroups_to_remove(ngs).unwrap().len(), 0);

        // x nodegroups, 1 ok, 2 create failed, 1 delete failure, others in other states => 4 to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder().nodegroup(nodegroup_ok).build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed)
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::CreateFailed)))
                        .set_status(Some(NodegroupStatus::CreateFailed))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Deleting)))
                        .set_status(Some(NodegroupStatus::Deleting))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Creating)))
                        .set_status(Some(NodegroupStatus::Creating))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Degraded)))
                        .set_status(Some(NodegroupStatus::Degraded))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::DeleteFailed)))
                        .set_status(Some(NodegroupStatus::DeleteFailed))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Deleting)))
                        .set_status(Some(NodegroupStatus::Deleting))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Updating)))
                        .set_status(Some(NodegroupStatus::Updating))
                        .build(),
                )
                .build(),
        ];
        let failed_ngs = check_failed_nodegroups_to_remove(ngs).unwrap();
        assert_eq!(failed_ngs.len(), 4);
        assert_eq!(
            failed_ngs[0].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_create_failed"
        );
        assert_eq!(
            failed_ngs[1].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_CreateFailed"
        );
        assert_eq!(
            failed_ngs[2].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_Degraded"
        );
        assert_eq!(
            failed_ngs[3].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_DeleteFailed"
        );
    }

    #[test]
    fn test_nodegroup_autoscaling_group() {
        let nodegroup_with_ds = |desired_nodes, enable_desired_nodes| {
            NodeGroupsWithDesiredState::new(
                "nodegroup".to_string(),
                None,
                3,
                10,
                desired_nodes,
                enable_desired_nodes,
                "t1000.xlarge".to_string(),
                20,
            )
        };
        let nodegroup = NodeGroups::new(
            "nodegroup".to_string(),
            3,
            10,
            "t1000.xlarge".to_string(),
            20,
            CpuArchitecture::AMD64,
        )
        .unwrap();

        // bootstrap
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Bootstrap, &nodegroup),
            nodegroup_with_ds(3, true) // need true because it's required from AWS to set desired node when initializing the autoscaler
        );
        // pause
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Pause, &nodegroup),
            nodegroup_with_ds(3, false)
        );
        // delete
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Delete, &nodegroup),
            nodegroup_with_ds(3, false)
        );
        // resume
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Resume(Some(5)), &nodegroup),
            nodegroup_with_ds(5, true)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Resume(None), &nodegroup),
            // if no info is given during resume, we should take the max and let the autoscaler reduce afterwards
            // but by setting it to the max, some users with have to ask support to raise limits
            // also useful when a customer wants to try Qovery, and do not need to ask AWS support in the early phase
            nodegroup_with_ds(3, true)
        );
        // update (we never have to change desired state during an update because the autoscaler manages it already)
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(6)), &nodegroup),
            nodegroup_with_ds(6, false)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(None), &nodegroup),
            nodegroup_with_ds(10, true) // max node is set just in case there is an issue with the AWS autoscaler to retrieve info, but should not be applied
        );
        // upgrade (we never have to change desired state during an update because the autoscaler manages it already)
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Upgrade(Some(7)), &nodegroup),
            nodegroup_with_ds(7, false)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(None), &nodegroup),
            nodegroup_with_ds(10, true) // max node is set just in case there is an issue with the AWS autoscaler to retrieve info, but should not be applied
        );

        // test autocorrection of silly stuffs
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(1)), &nodegroup),
            nodegroup_with_ds(3, true) // set to minimum if desired is below min
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(1000)), &nodegroup),
            nodegroup_with_ds(10, true) // set to max if desired is above max
        );
    }

    #[test]
    fn test_allowed_eks_nodes() {
        let event_details = EventDetails::new(
            None,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            Uuid::new_v4().to_string(),
            Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::Kubernetes(Uuid::new_v4(), "".to_string()),
        );
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3.medium".to_string(), 20, CpuArchitecture::AMD64).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3a.medium".to_string(), 20, CpuArchitecture::AMD64).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3.large".to_string(), 20, CpuArchitecture::AMD64).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3a.large".to_string(), 20, CpuArchitecture::AMD64).unwrap()],
            &event_details,
        )
        .is_ok());
        assert_eq!(
            EKS::validate_node_groups(
                vec![
                    NodeGroups::new("".to_string(), 3, 5, "t3.small".to_string(), 20, CpuArchitecture::AMD64).unwrap()
                ],
                &event_details,
            )
            .unwrap_err()
            .tag(),
            &Tag::NotAllowedInstanceType
        );
        assert_eq!(
            EKS::validate_node_groups(
                vec![
                    NodeGroups::new("".to_string(), 3, 5, "t3a.small".to_string(), 20, CpuArchitecture::AMD64).unwrap()
                ],
                &event_details,
            )
            .unwrap_err()
            .tag(),
            &Tag::NotAllowedInstanceType
        );
        assert_eq!(
            EKS::validate_node_groups(
                vec![
                    NodeGroups::new("".to_string(), 3, 5, "t1000.terminator".to_string(), 20, CpuArchitecture::AMD64)
                        .unwrap()
                ],
                &event_details,
            )
            .unwrap_err()
            .tag(),
            &Tag::UnsupportedInstanceType
        );
    }
}
