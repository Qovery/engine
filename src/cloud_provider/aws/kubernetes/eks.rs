use crate::cloud_provider::aws::kubernetes;
use crate::cloud_provider::aws::kubernetes::node::AwsInstancesType;
use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use crate::cloud_provider::kubernetes::{
    send_progress_on_long_task, Kind, Kubernetes, KubernetesNodesType, KubernetesUpgradeStatus,
};
use crate::cloud_provider::models::{
    ClusterAdvancedSettingsModel, KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState,
};
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::CloudProvider;
use crate::cmd::kubectl::{kubectl_exec_scale_replicas, ScalingKind};
use crate::cmd::terraform::terraform_init_validate_plan_apply;
use crate::dns_provider::DnsProvider;
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep};
use crate::io_models::context::Context;
use crate::io_models::progress_listener::{Listener, Listeners, ListenersHelper};
use crate::io_models::Action;
use crate::logger::Logger;
use crate::object_storage::s3::S3;
use crate::object_storage::ObjectStorage;
use function_name::named;
use std::borrow::Borrow;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use super::{get_rusoto_eks_client, should_update_desired_nodes};

/// EKS kubernetes provider allowing to deploy an EKS cluster.
pub struct EKS {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    version: String,
    region: AwsRegion,
    zones: Vec<AwsZones>,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    s3: S3,
    nodes_groups: Vec<NodeGroups>,
    template_directory: String,
    options: Options,
    listeners: Listeners,
    logger: Box<dyn Logger>,
    advanced_settings: ClusterAdvancedSettingsModel,
}

impl EKS {
    pub fn new(
        context: Context,
        id: &str,
        long_id: Uuid,
        name: &str,
        version: &str,
        region: AwsRegion,
        zones: Vec<String>,
        cloud_provider: Arc<Box<dyn CloudProvider>>,
        dns_provider: Arc<Box<dyn DnsProvider>>,
        options: Options,
        nodes_groups: Vec<NodeGroups>,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettingsModel,
    ) -> Result<Self, EngineError> {
        let event_details = kubernetes::event_details(&**cloud_provider, id, name, &region, &context);
        let template_directory = format!("{}/aws/bootstrap", context.lib_root_dir());

        let aws_zones = kubernetes::aws_zones(zones, &region, &event_details)?;

        // ensure config is ok
        if let Err(e) = EKS::validate_node_groups(nodes_groups.clone(), &event_details) {
            logger.log(EngineEvent::Error(e.clone(), None));
            return Err(e);
        };

        let s3 = kubernetes::s3(
            &context,
            &region,
            &**cloud_provider,
            advanced_settings.registry_image_retention_time,
        );

        // copy listeners from CloudProvider
        let listeners = cloud_provider.listeners().clone();

        Ok(EKS {
            context,
            id: id.to_string(),
            long_id,
            name: name.to_string(),
            version: version.to_string(),
            region,
            zones: aws_zones,
            cloud_provider,
            dns_provider,
            s3,
            options,
            nodes_groups,
            template_directory,
            logger,
            listeners,
            advanced_settings,
        })
    }

    pub fn validate_node_groups(
        nodes_groups: Vec<NodeGroups>,
        event_details: &EventDetails,
    ) -> Result<(), EngineError> {
        for node_group in &nodes_groups {
            match AwsInstancesType::from_str(node_group.instance_type.as_str()) {
                Ok(x) => {
                    if !EKS::is_instance_allowed(x) {
                        let err = EngineError::new_not_allowed_instance_type(
                            event_details.clone(),
                            node_group.instance_type.as_str(),
                        );
                        return Err(err);
                    }
                }
                Err(e) => {
                    let err = EngineError::new_unsupported_instance_type(
                        event_details.clone(),
                        node_group.instance_type.as_str(),
                        e,
                    );
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    pub fn is_instance_allowed(instance_type: AwsInstancesType) -> bool {
        match instance_type {
            AwsInstancesType::T2Large => true,
            AwsInstancesType::T2Xlarge => true,
            AwsInstancesType::T3Small => false,
            AwsInstancesType::T3Medium => true,
            AwsInstancesType::T3Large => true,
            AwsInstancesType::T3Xlarge => true,
            AwsInstancesType::T32xlarge => true,
            AwsInstancesType::T3aSmall => false,
            AwsInstancesType::T3aMedium => true,
            AwsInstancesType::T3aLarge => true,
            AwsInstancesType::T3aXlarge => true,
            AwsInstancesType::T3a2xlarge => true,
            AwsInstancesType::M5large => true,
            AwsInstancesType::M5Xlarge => true,
            AwsInstancesType::M52Xlarge => true,
            AwsInstancesType::M54Xlarge => true,
        }
    }

    fn set_cluster_autoscaler_replicas(
        &self,
        event_details: EventDetails,
        replicas_count: u32,
    ) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Scaling cluster autoscaler to `{}`.", replicas_count)),
        ));
        let (kubeconfig_path, _) = self.get_kubeconfig_file()?;
        let selector = "cluster-autoscaler-aws-cluster-autoscaler";
        let namespace = "kube-system";
        kubectl_exec_scale_replicas(
            kubeconfig_path,
            self.cloud_provider().credentials_environment_variables(),
            namespace,
            ScalingKind::Deployment,
            selector,
            replicas_count,
        )
        .map_err(|e| {
            EngineError::new_k8s_scale_replicas(
                event_details.clone(),
                selector.to_string(),
                namespace.to_string(),
                replicas_count,
                e,
            )
        })?;

        Ok(())
    }

    fn cloud_provider_name(&self) -> &str {
        "aws"
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

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.version.as_str()
    }

    fn region(&self) -> &str {
        self.region.to_aws_format()
    }

    fn zone(&self) -> &str {
        ""
    }

    fn aws_zones(&self) -> Option<Vec<AwsZones>> {
        Some(self.zones.clone())
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        (*self.cloud_provider).borrow()
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        (*self.dns_provider).borrow()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.s3
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    #[named]
    fn on_create(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || {
            kubernetes::create(
                self,
                self.long_id,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
            )
        })
    }

    #[named]
    fn on_create_error(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || kubernetes::create_error(self))
    }

    fn upgrade_with_status(&self, kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        let listeners_helper = ListenersHelper::new(&self.listeners);

        self.send_to_customer(
            format!(
                "Start preparing EKS upgrade process {} cluster with id {}",
                self.name(),
                self.id()
            )
            .as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Start preparing EKS cluster upgrade process".to_string()),
        ));

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), self) {
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

        // generate terraform files and copy them into temp dir
        let mut context = kubernetes::tera_context(self, &self.zones, &node_groups_with_desired_states, &self.options)?;

        //
        // Upgrade master nodes
        //
        match &kubernetes_upgrade_status.required_upgrade_on {
            Some(KubernetesNodesType::Masters) => {
                self.send_to_customer(
                    format!("Start upgrading process for master nodes on {}/{}", self.name(), self.id()).as_str(),
                    &listeners_helper,
                );
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
                    temp_dir.as_str(),
                    context.clone(),
                ) {
                    return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                        event_details,
                        self.template_directory.to_string(),
                        temp_dir,
                        e,
                    ));
                }

                let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
                let common_bootstrap_charts = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
                if let Err(e) = crate::template::copy_non_template_files(
                    common_bootstrap_charts.as_str(),
                    common_charts_temp_dir.as_str(),
                ) {
                    return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                        event_details,
                        common_bootstrap_charts,
                        common_charts_temp_dir,
                        e,
                    ));
                }

                self.send_to_customer(
                    format!("Upgrading Kubernetes {} master nodes", self.name()).as_str(),
                    &listeners_helper,
                );
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Upgrading Kubernetes master nodes.".to_string()),
                ));

                match terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
                    Ok(_) => {
                        self.send_to_customer(
                            format!("Kubernetes {} master nodes have been successfully upgraded", self.name()).as_str(),
                            &listeners_helper,
                        );
                        self.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(
                                "Kubernetes master nodes have been successfully upgraded.".to_string(),
                            ),
                        ));
                    }
                    Err(e) => {
                        return Err(EngineError::new_terraform_error(event_details, e));
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

        if let Err(e) = self.delete_crashlooping_pods(
            None,
            None,
            Some(3),
            self.cloud_provider().credentials_environment_variables(),
            Infrastructure(InfrastructureStep::Upgrade),
        ) {
            self.logger().log(EngineEvent::Error(e.clone(), None));
            return Err(e);
        }

        //
        // Upgrade worker nodes
        //
        self.send_to_customer(
            format!("Preparing workers nodes for upgrade for Kubernetes cluster {}", self.name()).as_str(),
            &listeners_helper,
        );
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
            temp_dir.as_str(),
            context.clone(),
        ) {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir,
                e,
            ));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let common_bootstrap_charts = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        if let Err(e) =
            crate::template::copy_non_template_files(common_bootstrap_charts.as_str(), common_charts_temp_dir.as_str())
        {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                common_bootstrap_charts,
                common_charts_temp_dir,
                e,
            ));
        }

        self.send_to_customer(
            format!("Upgrading Kubernetes {} worker nodes", self.name()).as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Upgrading Kubernetes worker nodes.".to_string()),
        ));

        // Disable cluster autoscaler deployment
        self.set_cluster_autoscaler_replicas(event_details.clone(), 0)?;

        if let Err(e) = self.delete_crashlooping_pods(
            None,
            None,
            Some(3),
            self.cloud_provider().credentials_environment_variables(),
            Infrastructure(InfrastructureStep::Upgrade),
        ) {
            self.logger().log(EngineEvent::Error(e.clone(), None));
            return Err(e);
        }

        if let Err(e) = self.delete_completed_jobs(
            self.cloud_provider().credentials_environment_variables(),
            Infrastructure(InfrastructureStep::Upgrade),
        ) {
            self.logger().log(EngineEvent::Error(e.clone(), None));
            return Err(e);
        }

        match terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            Ok(_) => {
                // ensure all nodes are ready on Kubernetes
                match self.check_workers_on_create() {
                    Ok(_) => {
                        self.send_to_customer(
                            format!("Kubernetes {} nodes have been successfully upgraded", self.name()).as_str(),
                            &listeners_helper,
                        );
                        self.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe("Kubernetes nodes have been successfully upgraded".to_string()),
                        ))
                    }
                    Err(e) => {
                        return Err(EngineError::new_k8s_node_not_ready(event_details, e));
                    }
                };
            }
            Err(e) => {
                // enable cluster autoscaler deployment
                self.set_cluster_autoscaler_replicas(event_details.clone(), 1)?;

                return Err(EngineError::new_terraform_error(event_details, e));
            }
        }

        // enable cluster autoscaler deployment
        self.set_cluster_autoscaler_replicas(event_details, 1)
    }

    #[named]
    fn on_upgrade(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || self.upgrade())
    }

    #[named]
    fn on_upgrade_error(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || kubernetes::upgrade_error(self))
    }

    #[named]
    fn on_downgrade(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Downgrade));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, kubernetes::downgrade)
    }

    #[named]
    fn on_downgrade_error(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Downgrade));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || kubernetes::downgrade_error(self))
    }

    #[named]
    fn on_pause(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || {
            kubernetes::pause(
                self,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
            )
        })
    }

    #[named]
    fn on_pause_error(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || kubernetes::pause_error(self))
    }

    #[named]
    fn on_delete(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || {
            kubernetes::delete(
                self,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
            )
        })
    }

    #[named]
    fn on_delete_error(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || kubernetes::delete_error(self))
    }

    fn get_advanced_settings(&self) -> ClusterAdvancedSettingsModel {
        self.advanced_settings.clone()
    }
}

#[allow(dead_code)] // used in tests
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
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::eks::{select_nodegroups_autoscaling_group_behavior, EKS};
    use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState};
    use crate::errors::Tag;
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;

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
        let nodegroup = NodeGroups::new("nodegroup".to_string(), 3, 10, "t1000.xlarge".to_string(), 20).unwrap();

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
            QoveryIdentifier::new_random(),
            None,
            Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::Kubernetes("".to_string(), "".to_string()),
        );
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3.medium".to_string(), 20).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3a.medium".to_string(), 20).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3.large".to_string(), 20).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3a.large".to_string(), 20).unwrap()],
            &event_details,
        )
        .is_ok());
        assert_eq!(
            EKS::validate_node_groups(
                vec![NodeGroups::new("".to_string(), 3, 5, "t3.small".to_string(), 20).unwrap()],
                &event_details
            )
            .unwrap_err()
            .tag(),
            &Tag::NotAllowedInstanceType
        );
        assert_eq!(
            EKS::validate_node_groups(
                vec![NodeGroups::new("".to_string(), 3, 5, "t3a.small".to_string(), 20).unwrap()],
                &event_details
            )
            .unwrap_err()
            .tag(),
            &Tag::NotAllowedInstanceType
        );
        assert_eq!(
            EKS::validate_node_groups(
                vec![NodeGroups::new("".to_string(), 3, 5, "t1000.terminator".to_string(), 20).unwrap()],
                &event_details
            )
            .unwrap_err()
            .tag(),
            &Tag::UnsupportedInstanceType
        );
    }
}
