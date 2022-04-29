use crate::cloud_provider;
use crate::cloud_provider::aws::kubernetes;
use crate::cloud_provider::aws::kubernetes::node::AwsInstancesType;
use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{
    send_progress_on_long_task, Kind, Kubernetes, KubernetesNodesType, KubernetesUpgradeStatus,
};
use crate::cloud_provider::models::NodeGroups;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::CloudProvider;
use crate::cmd::kubectl::{kubectl_exec_scale_replicas, ScalingKind};
use crate::cmd::terraform::terraform_init_validate_plan_apply;
use crate::dns_provider::DnsProvider;
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, InfrastructureStep, Stage};
use crate::io_models::{Action, Context, Listen, Listener, Listeners, ListenersHelper};
use crate::logger::Logger;
use crate::object_storage::s3::S3;
use crate::object_storage::ObjectStorage;
use function_name::named;
use std::borrow::Borrow;
use std::str::FromStr;
use std::sync::Arc;

/// EKS kubernetes provider allowing to deploy an EKS cluster.
pub struct EKS {
    context: Context,
    id: String,
    long_id: uuid::Uuid,
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
}

impl EKS {
    pub fn new(
        context: Context,
        id: &str,
        long_id: uuid::Uuid,
        name: &str,
        version: &str,
        region: AwsRegion,
        zones: Vec<String>,
        cloud_provider: Arc<Box<dyn CloudProvider>>,
        dns_provider: Arc<Box<dyn DnsProvider>>,
        options: Options,
        nodes_groups: Vec<NodeGroups>,
        logger: Box<dyn Logger>,
    ) -> Result<Self, EngineError> {
        let event_details = kubernetes::event_details(&**cloud_provider, id, name, &region, &context);
        let template_directory = format!("{}/aws/bootstrap", context.lib_root_dir());

        let aws_zones = kubernetes::aws_zones(zones, &region, &event_details)?;

        for node_group in &nodes_groups {
            if let Err(e) = AwsInstancesType::from_str(node_group.instance_type.as_str()) {
                let err =
                    EngineError::new_unsupported_instance_type(event_details, node_group.instance_type.as_str(), e);

                logger.log(EngineEvent::Error(err.clone(), None));

                return Err(err);
            }
        }

        let s3 = kubernetes::s3(&context, &region, &**cloud_provider);

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
        })
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
        let _ = kubectl_exec_scale_replicas(
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

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.version.as_str()
    }

    fn region(&self) -> String {
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

    #[named]
    fn on_create(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
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

        // generate terraform files and copy them into temp dir
        let mut context = kubernetes::tera_context(self, &self.zones, &self.nodes_groups, &self.options)?;

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
                        return Err(EngineError::new_terraform_error_while_executing_pipeline(event_details, e));
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
            Stage::Infrastructure(InfrastructureStep::Upgrade),
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
        let _ = self.set_cluster_autoscaler_replicas(event_details.clone(), 0)?;

        match terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            Ok(_) => {
                self.send_to_customer(
                    format!("Kubernetes {} workers nodes have been successfully upgraded", self.name()).as_str(),
                    &listeners_helper,
                );
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(
                        "Kubernetes workers nodes have been successfully upgraded.".to_string(),
                    ),
                ));
            }
            Err(e) => {
                // enable cluster autoscaler deployment
                let _ = self.set_cluster_autoscaler_replicas(event_details.clone(), 1)?;

                return Err(EngineError::new_terraform_error_while_executing_pipeline(event_details, e));
            }
        }

        // enable cluster autoscaler deployment
        self.set_cluster_autoscaler_replicas(event_details, 1)
    }

    #[named]
    fn on_upgrade(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade));
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade));
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Downgrade));
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Downgrade));
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
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

    #[named]
    fn deploy_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );
        cloud_provider::kubernetes::deploy_environment(self, environment, event_details, self.logger())
    }

    #[named]
    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );
        cloud_provider::kubernetes::deploy_environment_error(self, environment, event_details, self.logger())
    }

    #[named]
    fn pause_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );
        cloud_provider::kubernetes::pause_environment(self, environment, event_details, self.logger())
    }

    #[named]
    fn pause_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        Ok(())
    }

    #[named]
    fn delete_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );
        cloud_provider::kubernetes::delete_environment(self, environment, event_details, self.logger())
    }

    #[named]
    fn delete_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        Ok(())
    }
}

impl Listen for EKS {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
