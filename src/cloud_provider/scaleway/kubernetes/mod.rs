mod helm_charts;
pub mod node;

use crate::cloud_provider::aws::regions::AwsZones;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::helm::{deploy_charts_levels, ChartInfo};
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, send_progress_on_long_task, uninstall_cert_manager, Kind, Kubernetes,
    KubernetesUpgradeStatus, ProviderOptions,
};
use crate::cloud_provider::models::{NodeGroups, NodeGroupsFormat};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::scaleway::kubernetes::helm_charts::{scw_helm_charts, ChartsConfigPrerequisites};
use crate::cloud_provider::scaleway::kubernetes::node::{ScwInstancesType, ScwNodeGroup};
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::cmd::kubectl::{kubectl_exec_api_custom_metrics, kubectl_exec_get_all_namespaces, kubectl_exec_get_events};
use crate::cmd::kubectl_utils::kubectl_are_qovery_infra_pods_executed;
use crate::cmd::terraform::{
    terraform_apply_with_tf_workers_resources, terraform_init_validate_plan_apply, terraform_init_validate_state_list,
};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
use crate::io_models::context::{Context, Features};
use crate::io_models::domain::ToHelmString;
use crate::io_models::progress_listener::{Listener, Listeners, ListenersHelper};
use crate::io_models::{Action, QoveryIdentifier};
use crate::logger::Logger;
use crate::models::scaleway::ScwZone;
use crate::object_storage::scaleway_object_storage::{BucketDeleteStrategy, ScalewayOS};
use crate::object_storage::ObjectStorage;
use crate::runtime::block_on;
use crate::string::terraform_list_format;
use ::function_name::named;
use reqwest::StatusCode;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;
use scaleway_api_rs::apis::Error;
use scaleway_api_rs::models::ScalewayK8sV1Cluster;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tera::Context as TeraContext;
use uuid::Uuid;

#[derive(PartialEq)]
pub enum ScwNodeGroupErrors {
    CloudProviderApiError(CommandError),
    ClusterDoesNotExists(CommandError),
    MultipleClusterFound,
    NoNodePoolFound(CommandError),
    MissingNodePoolInfo,
    NodeGroupValidationError(CommandError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KapsuleOptions {
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    pub jwt_token: String,
    pub qovery_nats_url: String,
    pub qovery_nats_user: String,
    pub qovery_nats_password: String,
    pub qovery_ssh_key: String,
    #[serde(default)]
    pub user_ssh_keys: Vec<String>,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub agent_version_controller_token: String,
    pub qovery_engine_location: EngineLocation,
    pub engine_version_controller_token: String,

    // Scaleway
    pub scaleway_project_id: String,
    pub scaleway_access_key: String,
    pub scaleway_secret_key: String,

    // Other
    pub tls_email_report: String,
}

impl ProviderOptions for KapsuleOptions {}

impl KapsuleOptions {
    pub fn new(
        qovery_api_url: String,
        qovery_grpc_url: String,
        qoverry_cluster_jwt_token: String,
        qovery_nats_url: String,
        qovery_nats_user: String,
        qovery_nats_password: String,
        qovery_ssh_key: String,
        grafana_admin_user: String,
        grafana_admin_password: String,
        agent_version_controller_token: String,
        qovery_engine_location: EngineLocation,
        engine_version_controller_token: String,
        scaleway_project_id: String,
        scaleway_access_key: String,
        scaleway_secret_key: String,
        tls_email_report: String,
    ) -> KapsuleOptions {
        KapsuleOptions {
            qovery_api_url,
            qovery_grpc_url,
            jwt_token: qoverry_cluster_jwt_token,
            qovery_nats_url,
            qovery_nats_user,
            qovery_nats_password,
            qovery_ssh_key,
            user_ssh_keys: vec![],
            grafana_admin_user,
            grafana_admin_password,
            agent_version_controller_token,
            qovery_engine_location,
            engine_version_controller_token,
            scaleway_project_id,
            scaleway_access_key,
            scaleway_secret_key,
            tls_email_report,
        }
    }
}

pub struct Kapsule {
    context: Context,
    id: String,
    long_id: uuid::Uuid,
    name: String,
    version: String,
    zone: ScwZone,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    object_storage: ScalewayOS,
    nodes_groups: Vec<NodeGroups>,
    template_directory: String,
    options: KapsuleOptions,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl Kapsule {
    pub fn new(
        context: Context,
        id: String,
        long_id: uuid::Uuid,
        name: String,
        version: String,
        zone: ScwZone,
        cloud_provider: Arc<Box<dyn CloudProvider>>,
        dns_provider: Arc<Box<dyn DnsProvider>>,
        nodes_groups: Vec<NodeGroups>,
        options: KapsuleOptions,
        logger: Box<dyn Logger>,
    ) -> Result<Kapsule, EngineError> {
        let template_directory = format!("{}/scaleway/bootstrap", context.lib_root_dir());

        for node_group in &nodes_groups {
            if let Err(e) = ScwInstancesType::from_str(node_group.instance_type.as_str()) {
                let err = EngineError::new_unsupported_instance_type(
                    EventDetails::new(
                        Some(cloud_provider.kind()),
                        QoveryIdentifier::new_from_long_id(context.organization_id().to_string()),
                        QoveryIdentifier::new_from_long_id(context.cluster_id().to_string()),
                        QoveryIdentifier::new_from_long_id(context.execution_id().to_string()),
                        Some(zone.region_str().to_string()),
                        Infrastructure(InfrastructureStep::LoadConfiguration),
                        Transmitter::Kubernetes(id, name),
                    ),
                    node_group.instance_type.as_str(),
                    e,
                );

                logger.log(EngineEvent::Error(err.clone(), None));

                return Err(err);
            }
        }

        let object_storage = ScalewayOS::new(
            context.clone(),
            "s3-temp-id".to_string(),
            "default-s3".to_string(),
            cloud_provider.access_key_id(),
            cloud_provider.secret_access_key(),
            zone,
            BucketDeleteStrategy::Empty,
            false,
            context.resource_expiration_in_seconds(),
        );

        let listeners = cloud_provider.listeners().clone();
        Ok(Kapsule {
            context,
            id,
            long_id,
            name,
            version,
            zone,
            cloud_provider,
            dns_provider,
            object_storage,
            nodes_groups,
            template_directory,
            options,
            logger,
            listeners,
        })
    }

    fn get_configuration(&self) -> scaleway_api_rs::apis::configuration::Configuration {
        scaleway_api_rs::apis::configuration::Configuration {
            api_key: Some(scaleway_api_rs::apis::configuration::ApiKey {
                key: self.options.scaleway_secret_key.clone(),
                prefix: None,
            }),
            ..scaleway_api_rs::apis::configuration::Configuration::default()
        }
    }

    fn get_scw_cluster_info(&self) -> Result<Option<ScalewayK8sV1Cluster>, EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));

        // get cluster info
        let cluster_info = match block_on(scaleway_api_rs::apis::clusters_api::list_clusters(
            &self.get_configuration(),
            self.region(),
            None,
            Some(self.options.scaleway_project_id.as_str()),
            None,
            None,
            None,
            Some(self.cluster_name().as_str()),
            None,
            None,
        )) {
            Ok(x) => x,
            Err(e) => {
                return Err(EngineError::new_cannot_get_cluster_error(
                    event_details,
                    CommandError::new(
                        "Error, wasn't able to retrieve SCW cluster information from the API.".to_string(),
                        Some(e.to_string()),
                        None,
                    ),
                ));
            }
        };

        // if no cluster exists
        let cluster_info_content = cluster_info.clusters.unwrap();
        if cluster_info_content.is_empty() {
            return Ok(None);
        } else if cluster_info_content.len() != 1_usize {
            return Err(EngineError::new_multiple_cluster_found_expected_one_error(
                event_details,
                CommandError::new_from_safe_message(format!(
                    "Error, too many clusters found ({}) with this name, where 1 was expected.",
                    &cluster_info_content.len()
                )),
            ));
        }

        Ok(Some(cluster_info_content[0].clone()))
    }

    fn get_existing_sanitized_node_groups(
        &self,
        cluster_info: ScalewayK8sV1Cluster,
    ) -> Result<Vec<ScwNodeGroup>, ScwNodeGroupErrors> {
        let error_cluster_id = "expected cluster id for this Scaleway cluster".to_string();
        let cluster_id = match cluster_info.id {
            None => {
                return Err(ScwNodeGroupErrors::NodeGroupValidationError(
                    CommandError::new_from_safe_message(error_cluster_id),
                ))
            }
            Some(x) => x,
        };

        let pools = match block_on(scaleway_api_rs::apis::pools_api::list_pools(
            &self.get_configuration(),
            self.region(),
            cluster_id.as_str(),
            None,
            None,
            None,
            None,
            None,
        )) {
            Ok(x) => x,
            Err(e) => {
                return Err(ScwNodeGroupErrors::CloudProviderApiError(CommandError::new(
                    format!("Error while trying to get SCW pool info from cluster {}.", &cluster_id),
                    Some(e.to_string()),
                    None,
                )));
            }
        };

        // ensure pool are present
        if pools.pools.is_none() {
            return Err(ScwNodeGroupErrors::NoNodePoolFound(CommandError::new_from_safe_message(
                format!(
                    "Error, no SCW pool found from the SCW API for cluster {}/{}",
                    &cluster_id,
                    &cluster_info.name.unwrap_or_else(|| "unknown cluster".to_string())
                ),
            )));
        }

        // create sanitized nodegroup pools
        let mut nodegroup_pool: Vec<ScwNodeGroup> = Vec::with_capacity(pools.total_count.unwrap_or(0 as f32) as usize);
        for ng in pools.pools.unwrap() {
            if ng.id.is_none() {
                return Err(ScwNodeGroupErrors::NodeGroupValidationError(
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to validate SCW pool ID from cluster {}",
                        &cluster_id
                    )),
                ));
            }
            let ng_sanitized = self.get_node_group_info(ng.id.unwrap().as_str())?;
            nodegroup_pool.push(ng_sanitized)
        }

        Ok(nodegroup_pool)
    }

    fn get_node_group_info(&self, pool_id: &str) -> Result<ScwNodeGroup, ScwNodeGroupErrors> {
        let pool =
            match block_on(scaleway_api_rs::apis::pools_api::get_pool(
                &self.get_configuration(),
                self.region(),
                pool_id,
            )) {
                Ok(x) => x,
                Err(e) => return Err(match e {
                    Error::ResponseError(x) => {
                        let msg_with_error =
                            format!("Error code while getting node group: {}, API message: {} ", x.status, x.content);
                        match x.status {
                            StatusCode::NOT_FOUND => ScwNodeGroupErrors::NoNodePoolFound(CommandError::new(
                                "No node pool found".to_string(),
                                Some(msg_with_error),
                                None,
                            )),
                            _ => ScwNodeGroupErrors::CloudProviderApiError(CommandError::new(
                                "Scaleway API error while trying to get node group".to_string(),
                                Some(msg_with_error),
                                None,
                            )),
                        }
                    }
                    _ => ScwNodeGroupErrors::NodeGroupValidationError(CommandError::new(
                        "This Scaleway API error is not supported in the engine, please add it to better support it"
                            .to_string(),
                        Some(e.to_string()),
                        None,
                    )),
                }),
            };

        // ensure there is no missing info
        if let Err(e) = self.check_missing_nodegroup_info(&pool.name, "name") {
            return Err(e);
        };
        if let Err(e) = self.check_missing_nodegroup_info(&pool.min_size, "min_size") {
            return Err(e);
        };
        if let Err(e) = self.check_missing_nodegroup_info(&pool.max_size, "max_size") {
            return Err(e);
        };
        if let Err(e) = self.check_missing_nodegroup_info(&pool.status, "status") {
            return Err(e);
        };

        match ScwNodeGroup::new(
            pool.id,
            pool.name.unwrap(),
            pool.min_size.unwrap() as i32,
            pool.max_size.unwrap() as i32,
            pool.node_type,
            pool.size as i32,
            pool.status.unwrap(),
        ) {
            Ok(x) => Ok(x),
            Err(e) => Err(ScwNodeGroupErrors::NodeGroupValidationError(e)),
        }
    }

    fn check_missing_nodegroup_info<T>(&self, item: &Option<T>, name: &str) -> Result<(), ScwNodeGroupErrors> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));

        if item.is_none() {
            self.logger.log(EngineEvent::Error(
                EngineError::new_missing_workers_group_info_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Missing node pool info {} for cluster {}",
                        name,
                        self.context.cluster_id()
                    )),
                ),
                None,
            ));
            return Err(ScwNodeGroupErrors::MissingNodePoolInfo);
        };

        Ok(())
    }

    fn kubeconfig_bucket_name(&self) -> String {
        format!("qovery-kubeconfigs-{}", self.id())
    }

    fn get_engine_location(&self) -> EngineLocation {
        self.options.qovery_engine_location.clone()
    }

    fn logs_bucket_name(&self) -> String {
        format!("qovery-logs-{}", self.id)
    }

    fn tera_context(&self) -> Result<TeraContext, EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        let mut context = TeraContext::new();

        // Scaleway
        context.insert("scaleway_project_id", self.options.scaleway_project_id.as_str());
        context.insert("scaleway_access_key", self.options.scaleway_access_key.as_str());
        context.insert("scaleway_secret_key", self.options.scaleway_secret_key.as_str());
        context.insert("scw_region", &self.zone.region().as_str());
        context.insert("scw_zone", &self.zone.as_str());

        // DNS
        let managed_dns_list = vec![self.dns_provider.name()];
        let managed_dns_domains_helm_format = vec![self.dns_provider.domain().to_string()];
        let managed_dns_domains_root_helm_format = vec![self.dns_provider.domain().root_domain().to_string()];
        let managed_dns_domains_terraform_format = terraform_list_format(vec![self.dns_provider.domain().to_string()]);
        let managed_dns_domains_root_terraform_format =
            terraform_list_format(vec![self.dns_provider.domain().root_domain().to_string()]);
        let managed_dns_resolvers_terraform_format = self.managed_dns_resolvers_terraform_format();

        context.insert("managed_dns", &managed_dns_list);
        context.insert("managed_dns_domains_helm_format", &managed_dns_domains_helm_format);
        context.insert("managed_dns_domains_root_helm_format", &managed_dns_domains_root_helm_format);
        context.insert("managed_dns_domains_terraform_format", &managed_dns_domains_terraform_format);
        context.insert(
            "managed_dns_domains_root_terraform_format",
            &managed_dns_domains_root_terraform_format,
        );
        context.insert(
            "managed_dns_resolvers_terraform_format",
            &managed_dns_resolvers_terraform_format,
        );
        context.insert("wildcard_managed_dns", &self.dns_provider().domain().wildcarded().to_string());

        // add specific DNS fields
        self.dns_provider().insert_into_teracontext(&mut context);

        context.insert("dns_email_report", &self.options.tls_email_report);

        // Kubernetes
        context.insert("test_cluster", &self.context.is_test_cluster());
        context.insert("kubernetes_full_cluster_id", &self.long_id);
        context.insert("kubernetes_cluster_id", self.id());
        context.insert("kubernetes_cluster_name", self.cluster_name().as_str());
        context.insert("kubernetes_cluster_version", self.version());

        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("object_storage_kubeconfig_bucket", &self.kubeconfig_bucket_name());
        context.insert("object_storage_logs_bucket", &self.logs_bucket_name());

        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());
        context.insert("qovery_nats_url", self.options.qovery_nats_url.as_str());
        context.insert("qovery_nats_user", self.options.qovery_nats_user.as_str());
        context.insert("qovery_nats_password", self.options.qovery_nats_password.as_str());
        context.insert("engine_version_controller_token", &self.options.engine_version_controller_token);
        context.insert("agent_version_controller_token", &self.options.agent_version_controller_token);

        // Qovery features
        context.insert("log_history_enabled", &self.context.is_feature_enabled(&Features::LogsHistory));
        context.insert(
            "metrics_history_enabled",
            &self.context.is_feature_enabled(&Features::MetricsHistory),
        );
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert("resource_expiration_in_seconds", &self.context.resource_expiration_in_seconds())
        }

        // AWS S3 tfstates storage tfstates
        context.insert(
            "aws_access_key_tfstates_account",
            self.cloud_provider()
                .terraform_state_credentials()
                .access_key_id
                .as_str(),
        );
        context.insert(
            "aws_secret_key_tfstates_account",
            self.cloud_provider()
                .terraform_state_credentials()
                .secret_access_key
                .as_str(),
        );
        context.insert(
            "aws_region_tfstates_account",
            self.cloud_provider().terraform_state_credentials().region.as_str(),
        );
        context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");
        context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");

        // TLS
        context.insert("acme_server_url", &self.lets_encrypt_url());

        // Vault
        context.insert("vault_auth_method", "none");

        if env::var_os("VAULT_ADDR").is_some() {
            // select the correct used method
            match env::var_os("VAULT_ROLE_ID") {
                Some(role_id) => {
                    context.insert("vault_auth_method", "app_role");
                    context.insert("vault_role_id", role_id.to_str().unwrap());

                    match env::var_os("VAULT_SECRET_ID") {
                        Some(secret_id) => context.insert("vault_secret_id", secret_id.to_str().unwrap()),
                        None => self.logger().log(EngineEvent::Error(
                            EngineError::new_missing_required_env_variable(
                                event_details,
                                "VAULT_SECRET_ID".to_string(),
                            ),
                            None,
                        )),
                    }
                }
                None => {
                    if env::var_os("VAULT_TOKEN").is_some() {
                        context.insert("vault_auth_method", "token")
                    }
                }
            }
        };

        // grafana credentials
        context.insert("grafana_admin_user", self.options.grafana_admin_user.as_str());
        context.insert("grafana_admin_password", self.options.grafana_admin_password.as_str());

        // Kubernetes workers
        context.insert("scw_ks_worker_nodes", &self.nodes_groups);
        context.insert("scw_ks_pool_autoscale", &true);

        Ok(context)
    }

    fn managed_dns_resolvers_terraform_format(&self) -> String {
        let managed_dns_resolvers: Vec<String> = self
            .dns_provider
            .resolvers()
            .iter()
            .map(|x| x.clone().to_string())
            .collect();

        terraform_list_format(managed_dns_resolvers)
    }

    fn lets_encrypt_url(&self) -> String {
        match &self.context.is_test_cluster() {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        }
        .to_string()
    }

    fn create(&self) -> Result<(), EngineError> {
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));

        // TODO(DEV-1061): remove legacy logger
        self.send_to_customer(
            format!("Preparing SCW {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing SCW cluster deployment.".to_string()),
        ));

        // upgrade cluster instead if required
        match self.get_kubeconfig_file() {
            Ok((path, _)) => match is_kubernetes_upgrade_required(
                path,
                &self.version,
                self.cloud_provider.credentials_environment_variables(),
                event_details.clone(),
                self.logger(),
            ) {
                Ok(x) => {
                    if x.required_upgrade_on.is_some() {
                        return self.upgrade_with_status(x);
                    }

                    self.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                    ))
                }
                Err(e) => {
                    self.logger().log(EngineEvent::Error(e, Some(EventMessage::new_from_safe(
                        "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                    ))));
                }
            },
            Err(_) => self.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Kubernetes cluster upgrade not required, config file is not found and cluster have certainly never been deployed before".to_string())))

        };

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let context = self.tera_context()?;

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            context,
        ) {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir,
                e,
            ));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str())
        {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                bootstrap_charts_dir,
                common_charts_temp_dir,
                e,
            ));
        }

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deploying SCW cluster.".to_string()),
        ));

        self.send_to_customer(
            format!("Deploying SCW {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        // TODO(benjaminch): move this elsewhere
        // Create object-storage buckets
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Create Qovery managed object storage buckets".to_string()),
        ));
        if let Err(e) = self
            .object_storage
            .create_bucket(self.kubeconfig_bucket_name().as_str())
        {
            let error = EngineError::new_object_storage_cannot_create_bucket_error(
                event_details,
                self.kubeconfig_bucket_name(),
                e,
            );
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(error);
        }

        // Logs bucket
        if let Err(e) = self.object_storage.create_bucket(self.logs_bucket_name().as_str()) {
            let error =
                EngineError::new_object_storage_cannot_create_bucket_error(event_details, self.logs_bucket_name(), e);
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(error);
        }

        // terraform deployment dedicated to cloud resources
        if let Err(e) = terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            return Err(EngineError::new_terraform_error(event_details, e));
        }

        // push config file to object storage
        let kubeconfig_path = &self.get_kubeconfig_file_path()?;
        let kubeconfig_path = Path::new(kubeconfig_path);
        let kubeconfig_name = self.get_kubeconfig_filename();
        if let Err(e) = self.object_storage.put(
            self.kubeconfig_bucket_name().as_str(),
            kubeconfig_name.as_str(),
            kubeconfig_path.to_str().expect("No path for Kubeconfig"),
        ) {
            let error = EngineError::new_object_storage_cannot_put_file_into_bucket_error(
                event_details,
                self.logs_bucket_name(),
                kubeconfig_name.to_string(),
                e,
            );
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(error);
        }

        let cluster_info = self.get_scw_cluster_info()?;
        if cluster_info.is_none() {
            return Err(EngineError::new_no_cluster_found_error(
                event_details,
                CommandError::new_from_safe_message("Error, no cluster found from the Scaleway API".to_string()),
            ));
        }

        let current_nodegroups = match self
            .get_existing_sanitized_node_groups(cluster_info.expect("A cluster should be present at this create stage"))
        {
            Ok(x) => x,
            Err(e) => {
                match e {
                    ScwNodeGroupErrors::CloudProviderApiError(c) => {
                        return Err(EngineError::new_missing_api_info_from_cloud_provider_error(
                            event_details,
                            Some(c),
                        ))
                    }
                    ScwNodeGroupErrors::ClusterDoesNotExists(_) => self.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new_from_safe(
                            "Cluster do not exists, no node groups can be retrieved for upgrade check.".to_string(),
                        ),
                    )),
                    ScwNodeGroupErrors::MultipleClusterFound => {
                        return Err(EngineError::new_multiple_cluster_found_expected_one_error(
                            event_details,
                            CommandError::new_from_safe_message(
                                "Error, multiple clusters found, can't match the correct node groups.".to_string(),
                            ),
                        ));
                    }
                    ScwNodeGroupErrors::NoNodePoolFound(_) => self.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new_from_safe(
                            "Cluster exists, but no node groups found for upgrade check.".to_string(),
                        ),
                    )),
                    ScwNodeGroupErrors::MissingNodePoolInfo => {
                        return Err(EngineError::new_missing_api_info_from_cloud_provider_error(
                            event_details,
                            Some(CommandError::new_from_safe_message(
                                "Error with Scaleway API while trying to retrieve node pool info".to_string(),
                            )),
                        ));
                    }
                    ScwNodeGroupErrors::NodeGroupValidationError(c) => {
                        return Err(EngineError::new_missing_api_info_from_cloud_provider_error(
                            event_details,
                            Some(c),
                        ));
                    }
                };
                Vec::with_capacity(0)
            }
        };

        // ensure all node groups are in ready state Scaleway side
        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(
                "Ensuring all groups nodes are in ready state from the Scaleway API".to_string(),
            ),
        ));

        for ng in current_nodegroups {
            let res = retry::retry(
                // retry 10 min max per nodegroup until they are ready
                Fixed::from_millis(15000).take(40),
                || {
                    self.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!(
                            "checking node group {}/{:?}, current status: {:?}",
                            &ng.name,
                            &ng.id.as_ref().unwrap_or(&"unknown".to_string()),
                            &ng.status
                        )),
                    ));
                    let pool_id = match &ng.id {
                        None => {
                            let msg =
                                "node group id was expected to get info, but not found from Scaleway API".to_string();
                            return OperationResult::Retry(
                                EngineError::new_missing_api_info_from_cloud_provider_error(
                                    event_details.clone(),
                                    Some(CommandError::new_from_safe_message(msg)),
                                ),
                            );
                        }
                        Some(x) => x,
                    };
                    let scw_ng = match self.get_node_group_info(pool_id.as_str()) {
                        Ok(x) => x,
                        Err(e) => {
                            return match e {
                                ScwNodeGroupErrors::CloudProviderApiError(c) => {
                                    let current_error = EngineError::new_missing_api_info_from_cloud_provider_error(
                                        event_details.clone(),
                                        Some(c),
                                    );
                                    self.logger.log(EngineEvent::Error(current_error.clone(), None));
                                    OperationResult::Retry(current_error)
                                }
                                ScwNodeGroupErrors::ClusterDoesNotExists(c) => {
                                    let current_error =
                                        EngineError::new_no_cluster_found_error(event_details.clone(), c);
                                    self.logger.log(EngineEvent::Error(current_error.clone(), None));
                                    OperationResult::Retry(current_error)
                                }
                                ScwNodeGroupErrors::MultipleClusterFound => {
                                    OperationResult::Retry(EngineError::new_multiple_cluster_found_expected_one_error(
                                        event_details.clone(),
                                        CommandError::new_from_safe_message(
                                            "Multiple cluster found while one was expected".to_string(),
                                        ),
                                    ))
                                }
                                ScwNodeGroupErrors::NoNodePoolFound(_) => OperationResult::Ok(()),
                                ScwNodeGroupErrors::MissingNodePoolInfo => {
                                    OperationResult::Retry(EngineError::new_missing_api_info_from_cloud_provider_error(
                                        event_details.clone(),
                                        None,
                                    ))
                                }
                                ScwNodeGroupErrors::NodeGroupValidationError(c) => {
                                    let current_error = EngineError::new_missing_api_info_from_cloud_provider_error(
                                        event_details.clone(),
                                        Some(c),
                                    );
                                    self.logger.log(EngineEvent::Error(current_error.clone(), None));
                                    OperationResult::Retry(current_error)
                                }
                            }
                        }
                    };
                    match scw_ng.status == scaleway_api_rs::models::scaleway_k8s_v1_pool::Status::Ready {
                        true => OperationResult::Ok(()),
                        false => OperationResult::Retry(EngineError::new_k8s_node_not_ready(
                            event_details.clone(),
                            CommandError::new_from_safe_message(format!(
                                "waiting for node group {} to be ready, current status: {:?}",
                                &scw_ng.name, scw_ng.status
                            )),
                        )),
                    }
                },
            );
            match res {
                Ok(_) => {}
                Err(Operation { error, .. }) => return Err(error),
                Err(retry::Error::Internal(msg)) => {
                    return Err(EngineError::new_k8s_node_not_ready(
                        event_details,
                        CommandError::new("Waiting for too long worker nodes to be ready".to_string(), Some(msg), None),
                    ))
                }
            }
        }
        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(
                "All node groups for this cluster are ready from cloud provider API".to_string(),
            ),
        ));

        // ensure all nodes are ready on Kubernetes
        match self.check_workers_on_create() {
            Ok(_) => {
                self.send_to_customer(
                    format!("Kubernetes {} nodes have been successfully created", self.name()).as_str(),
                    &listeners_helper,
                );
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Kubernetes nodes have been successfully created".to_string()),
                ))
            }
            Err(e) => {
                return Err(EngineError::new_k8s_node_not_ready(event_details, e));
            }
        };

        // kubernetes helm deployments on the cluster
        let credentials_environment_variables: Vec<(String, String)> = self
            .cloud_provider
            .credentials_environment_variables()
            .into_iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect();

        let charts_prerequisites = ChartsConfigPrerequisites::new(
            self.cloud_provider.organization_id().to_string(),
            self.cloud_provider.organization_long_id(),
            self.id().to_string(),
            self.long_id,
            self.zone,
            self.cluster_name(),
            "scw".to_string(),
            self.context.is_test_cluster(),
            self.cloud_provider.access_key_id(),
            self.cloud_provider.secret_access_key(),
            self.options.scaleway_project_id.to_string(),
            self.options.qovery_engine_location.clone(),
            self.context.is_feature_enabled(&Features::LogsHistory),
            self.context.is_feature_enabled(&Features::MetricsHistory),
            self.dns_provider.domain().root_domain().to_string(),
            self.dns_provider.domain().to_helm_format_string(),
            self.managed_dns_resolvers_terraform_format(),
            self.dns_provider.provider_name().to_string(),
            self.options.tls_email_report.clone(),
            self.lets_encrypt_url(),
            self.dns_provider().provider_configuration(),
            self.context.disable_pleco(),
            self.options.clone(),
        );

        if let Err(e) = kubectl_are_qovery_infra_pods_executed(kubeconfig_path, &credentials_environment_variables) {
            self.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new("Didn't manage to restart all paused pods".to_string(), Some(e.to_string())),
            ));
        }

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
        ));
        let helm_charts_to_deploy = scw_helm_charts(
            format!("{}/qovery-tf-config.json", &temp_dir).as_str(),
            &charts_prerequisites,
            Some(&temp_dir),
            kubeconfig_path,
            &credentials_environment_variables,
        )
        .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?;

        deploy_charts_levels(
            kubeconfig_path,
            &credentials_environment_variables,
            helm_charts_to_deploy,
            self.context.is_dry_run_deploy(),
        )
        .map_err(|e| EngineError::new_helm_charts_deploy_error(event_details.clone(), e))
    }

    fn create_error(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        let (kubeconfig_path, _) = self.get_kubeconfig_file()?;
        let environment_variables: Vec<(&str, &str)> = self.cloud_provider.credentials_environment_variables();

        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Infrastructure(InfrastructureStep::Create)),
            EventMessage::new_from_safe("SCW.create_error() called.".to_string()),
        ));

        match kubectl_exec_get_events(kubeconfig_path, None, environment_variables) {
            Ok(ok_line) => self
                .logger()
                .log(EngineEvent::Info(event_details, EventMessage::new_from_safe(ok_line))),
            Err(err) => self.logger().log(EngineEvent::Warning(
                event_details,
                EventMessage::new(
                    "Error trying to get kubernetes events".to_string(),
                    Some(err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                ),
            )),
        };

        Ok(())
    }

    fn upgrade_error(&self) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Infrastructure(InfrastructureStep::Upgrade)),
            EventMessage::new_from_safe("SCW.upgrade_error() called.".to_string()),
        ));

        Ok(())
    }

    fn downgrade(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn downgrade_error(&self) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Infrastructure(InfrastructureStep::Downgrade)),
            EventMessage::new_from_safe("SCW.downgrade_error() called.".to_string()),
        ));

        Ok(())
    }

    fn pause(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        let listeners_helper = ListenersHelper::new(&self.listeners);

        self.send_to_customer(
            format!("Preparing SCW {} cluster pause with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        self.logger().log(EngineEvent::Info(
            self.get_event_details(Infrastructure(InfrastructureStep::Pause)),
            EventMessage::new_from_safe("Preparing cluster pause.".to_string()),
        ));

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
        let scw_ks_worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
        context.insert("scw_ks_worker_nodes", &scw_ks_worker_nodes);

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            context,
        ) {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir,
                e,
            ));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str())
        {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                bootstrap_charts_dir,
                common_charts_temp_dir,
                e,
            ));
        }

        // pause: only select terraform workers elements to pause to avoid applying on the whole config
        // this to avoid failures because of helm deployments on removing workers nodes
        let tf_workers_resources = match terraform_init_validate_state_list(temp_dir.as_str()) {
            Ok(x) => {
                let mut tf_workers_resources_name = Vec::new();
                for name in x {
                    if name.starts_with("scaleway_k8s_pool.") {
                        tf_workers_resources_name.push(name);
                    }
                }
                tf_workers_resources_name
            }
            Err(e) => {
                let error = EngineError::new_terraform_error(event_details, e);
                self.logger().log(EngineEvent::Error(error.clone(), None));
                return Err(error);
            }
        };

        if tf_workers_resources.is_empty() {
            self.logger().log(EngineEvent::Warning(
                event_details,
                EventMessage::new_from_safe(
                    "Could not find workers resources in terraform state. Cluster seems already paused.".to_string(),
                ),
            ));
            return Ok(());
        }

        let kubernetes_config_file_path = self.get_kubeconfig_file_path()?;

        // pause: wait 1h for the engine to have 0 running jobs before pausing and avoid getting unreleased lock (from helm or terraform for example)
        if self.get_engine_location() == EngineLocation::ClientSide {
            match self.context.is_feature_enabled(&Features::MetricsHistory) {
                true => {
                    let metric_name = "taskmanager_nb_running_tasks";
                    let wait_engine_job_finish = retry::retry(Fixed::from_millis(60000).take(60), || {
                        return match kubectl_exec_api_custom_metrics(
                            &kubernetes_config_file_path,
                            self.cloud_provider().credentials_environment_variables(),
                            "qovery",
                            None,
                            metric_name,
                        ) {
                            Ok(metrics) => {
                                let mut current_engine_jobs = 0;

                                for metric in metrics.items {
                                    match metric.value.parse::<i32>() {
                                        Ok(job_count) if job_count > 0 => current_engine_jobs += 1,
                                        Err(e) => {
                                            return OperationResult::Retry(EngineError::new_cannot_get_k8s_api_custom_metrics(event_details.clone(), CommandError::new("Error while looking at the API metric value".to_string(), Some(e.to_string()), None)));
                                        }
                                        _ => {}
                                    }
                                }

                                if current_engine_jobs == 0 {
                                    OperationResult::Ok(())
                                } else {
                                    OperationResult::Retry(EngineError::new_cannot_pause_cluster_tasks_are_running(event_details.clone(), None))
                                }
                            }
                            Err(e) => {
                                OperationResult::Retry(
                                    EngineError::new_cannot_get_k8s_api_custom_metrics(event_details.clone(), e))
                            }
                        };
                    });

                    match wait_engine_job_finish {
                        Ok(_) => {
                            self.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("No current running jobs on the Engine, infrastructure pause is allowed to start".to_string())));
                        }
                        Err(Operation { error, .. }) => {
                            return Err(error)
                        }
                        Err(retry::Error::Internal(msg)) => {
                            return Err(EngineError::new_cannot_pause_cluster_tasks_are_running(event_details, Some(CommandError::new_from_safe_message(msg))))
                        }
                    }
                }
                false => self.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Engines are running Client side, but metric history flag is disabled. You will encounter issues during cluster lifecycles if you do not enable metric history".to_string()))),
            }
        }

        self.send_to_customer(
            format!("Pausing SCW {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Pausing cluster deployment.".to_string()),
        ));

        if let Err(e) = terraform_apply_with_tf_workers_resources(temp_dir.as_str(), tf_workers_resources) {
            return Err(EngineError::new_terraform_error(event_details, e));
        }

        if let Err(e) = self.check_workers_on_pause() {
            return Err(EngineError::new_k8s_node_not_ready(event_details, e));
        };

        let message = format!("Kubernetes cluster {} successfully paused", self.name());
        self.send_to_customer(&message, &listeners_helper);
        self.logger()
            .log(EngineEvent::Info(event_details, EventMessage::new_from_safe(message)));
        Ok(())
    }

    fn pause_error(&self) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Infrastructure(InfrastructureStep::Pause)),
            EventMessage::new_from_safe("SCW.pause_error() called.".to_string()),
        ));

        Ok(())
    }

    fn delete(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let skip_kubernetes_step = false;

        self.send_to_customer(
            format!("Preparing to delete SCW cluster {} with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing to delete cluster.".to_string()),
        ));

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let context = self.tera_context()?;

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            context,
        ) {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir,
                e,
            ));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str())
        {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                bootstrap_charts_dir,
                common_charts_temp_dir,
                e,
            ));
        }

        // should apply before destroy to be sure destroy will compute on all resources
        // don't exit on failure, it can happen if we resume a destroy process
        let message = format!(
            "Ensuring everything is up to date before deleting cluster {}/{}",
            self.name(),
            self.id()
        );
        self.send_to_customer(&message, &listeners_helper);
        self.logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Running Terraform apply before running a delete.".to_string()),
        ));

        if let Err(e) = terraform_init_validate_plan_apply(temp_dir.as_str(), false) {
            // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
            self.logger().log(EngineEvent::Error(
                EngineError::new_terraform_error(event_details.clone(), e),
                None,
            ));
        };

        let kubeconfig_path = &self.get_kubeconfig_file_path()?;
        let kubeconfig_path = Path::new(kubeconfig_path);

        if !skip_kubernetes_step {
            // should make the diff between all namespaces and qovery managed namespaces
            let message = format!(
                "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
                self.name(),
                self.id()
            );
            self.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(message.to_string()),
            ));
            self.send_to_customer(&message, &listeners_helper);

            let all_namespaces = kubectl_exec_get_all_namespaces(
                &kubeconfig_path,
                self.cloud_provider().credentials_environment_variables(),
            );

            match all_namespaces {
                Ok(namespace_vec) => {
                    let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                    let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                    self.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe("Deleting non Qovery namespaces".to_string()),
                    ));

                    for namespace_to_delete in namespaces_to_delete.iter() {
                        match cmd::kubectl::kubectl_exec_delete_namespace(
                            &kubeconfig_path,
                            namespace_to_delete,
                            self.cloud_provider().credentials_environment_variables(),
                        ) {
                            Ok(_) => self.logger().log(EngineEvent::Info(
                                event_details.clone(),
                                EventMessage::new_from_safe(format!(
                                    "Namespace `{}` deleted successfully.",
                                    namespace_to_delete
                                )),
                            )),
                            Err(e) => {
                                if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                                    self.logger().log(EngineEvent::Warning(
                                        event_details.clone(),
                                        EventMessage::new_from_safe(format!(
                                            "Can't delete the namespace `{}`",
                                            namespace_to_delete
                                        )),
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let message_safe = format!(
                        "Error while getting all namespaces for Kubernetes cluster {}",
                        self.name_with_id(),
                    );
                    self.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(
                            message_safe,
                            Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                        ),
                    ));
                }
            }

            let message = format!(
                "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
                self.name(),
                self.id()
            );
            self.send_to_customer(&message, &listeners_helper);
            self.logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

            // delete custom metrics api to avoid stale namespaces on deletion
            let helm = Helm::new(&kubeconfig_path, &self.cloud_provider.credentials_environment_variables())
                .map_err(|e| to_engine_error(&event_details, e))?;
            let chart = ChartInfo::new_from_release_name("metrics-server", "kube-system");
            helm.uninstall(&chart, &[])
                .map_err(|e| to_engine_error(&event_details, e))?;

            // required to avoid namespace stuck on deletion
            uninstall_cert_manager(
                &kubeconfig_path,
                self.cloud_provider().credentials_environment_variables(),
                event_details.clone(),
                self.logger(),
            )?;

            self.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe("Deleting Qovery managed helm charts".to_string()),
            ));

            let qovery_namespaces = get_qovery_managed_namespaces();
            for qovery_namespace in qovery_namespaces.iter() {
                let charts_to_delete = helm
                    .list_release(Some(qovery_namespace), &[])
                    .map_err(|e| to_engine_error(&event_details, e))?;

                for chart in charts_to_delete {
                    let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                    match helm.uninstall(&chart_info, &[]) {
                        Ok(_) => self.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                        )),
                        Err(e) => {
                            let message_safe = format!("Can't delete chart `{}`", chart.name);
                            self.logger().log(EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new(message_safe, Some(e.to_string())),
                            ))
                        }
                    }
                }
            }

            self.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe("Deleting Qovery managed namespaces".to_string()),
            ));

            for qovery_namespace in qovery_namespaces.iter() {
                let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                    &kubeconfig_path,
                    qovery_namespace,
                    self.cloud_provider().credentials_environment_variables(),
                );
                match deletion {
                    Ok(_) => self.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!("Namespace {} is fully deleted", qovery_namespace)),
                    )),
                    Err(e) => {
                        if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                            self.logger().log(EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new_from_safe(format!("Can't delete namespace {}.", qovery_namespace)),
                            ))
                        }
                    }
                }
            }

            self.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe("Delete all remaining deployed helm applications".to_string()),
            ));

            match helm.list_release(None, &[]) {
                Ok(helm_charts) => {
                    for chart in helm_charts {
                        let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                        match helm.uninstall(&chart_info, &[]) {
                            Ok(_) => self.logger().log(EngineEvent::Info(
                                event_details.clone(),
                                EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                            )),
                            Err(e) => {
                                let message_safe = format!("Error deleting chart `{}`", chart.name);
                                self.logger().log(EngineEvent::Warning(
                                    event_details.clone(),
                                    EventMessage::new(message_safe, Some(e.to_string())),
                                ))
                            }
                        }
                    }
                }
                Err(e) => {
                    let message_safe = "Unable to get helm list";
                    self.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(message_safe.to_string(), Some(e.to_string())),
                    ))
                }
            }
        };

        let message = format!("Deleting Kubernetes cluster {}/{}", self.name(), self.id());
        self.send_to_customer(&message, &listeners_helper);
        self.logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Running Terraform destroy".to_string()),
        ));

        match cmd::terraform::terraform_init_validate_destroy(temp_dir.as_str(), false) {
            Ok(_) => {
                self.send_to_customer(
                    format!("Kubernetes cluster {}/{} successfully deleted", self.name(), self.id()).as_str(),
                    &listeners_helper,
                );
                self.logger().log(EngineEvent::Info(
                    event_details,
                    EventMessage::new_from_safe("Kubernetes cluster successfully deleted".to_string()),
                ));
                Ok(())
            }
            Err(err) => Err(EngineError::new_terraform_error(event_details, err)),
        }
    }

    fn delete_error(&self) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Infrastructure(InfrastructureStep::Delete)),
            EventMessage::new_from_safe("SCW.delete_error() called.".to_string()),
        ));

        Ok(())
    }

    fn cloud_provider_name(&self) -> &str {
        "scaleway"
    }

    fn struct_name(&self) -> &str {
        "kubernetes"
    }
}

impl Kubernetes for Kapsule {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::ScwKapsule
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

    fn region(&self) -> &str {
        self.zone.region_str()
    }

    fn zone(&self) -> &str {
        self.zone.as_str()
    }

    fn aws_zones(&self) -> Option<Vec<AwsZones>> {
        None
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider.as_ref().borrow()
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider.as_ref().borrow()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.object_storage
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || self.create())
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
        send_progress_on_long_task(self, Action::Create, || self.create_error())
    }

    fn upgrade_with_status(&self, kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        let listeners_helper = ListenersHelper::new(&self.listeners);
        self.send_to_customer(
            format!(
                "Start preparing Kapsule upgrade process {} cluster with id {}",
                self.name(),
                self.id()
            )
            .as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Start preparing cluster upgrade process".to_string()),
        ));

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        //
        // Upgrade nodes
        //
        self.send_to_customer(
            format!("Preparing nodes for upgrade for Kubernetes cluster {}", self.name()).as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing nodes for upgrade for Kubernetes cluster.".to_string()),
        ));

        context.insert(
            "kubernetes_cluster_version",
            format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
        );

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            context,
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
            format!("Upgrading Kubernetes {} nodes", self.name()).as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Upgrading Kubernetes nodes.".to_string()),
        ));

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
            Ok(_) => match self.check_workers_on_upgrade(kubernetes_upgrade_status.requested_version.to_string()) {
                Ok(_) => {
                    self.send_to_customer(
                        format!("Kubernetes {} nodes have been successfully upgraded", self.name()).as_str(),
                        &listeners_helper,
                    );
                    self.logger().log(EngineEvent::Info(
                        event_details,
                        EventMessage::new_from_safe("Kubernetes nodes have been successfully upgraded.".to_string()),
                    ));
                }
                Err(e) => {
                    return Err(EngineError::new_k8s_node_not_ready_with_requested_version(
                        event_details,
                        kubernetes_upgrade_status.requested_version.to_string(),
                        e,
                    ));
                }
            },
            Err(e) => {
                return Err(EngineError::new_terraform_error(event_details, e));
            }
        }

        Ok(())
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
        send_progress_on_long_task(self, Action::Create, || self.upgrade_error())
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
        send_progress_on_long_task(self, Action::Create, || self.downgrade())
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
        send_progress_on_long_task(self, Action::Create, || self.downgrade_error())
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
        send_progress_on_long_task(self, Action::Pause, || self.pause())
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
        send_progress_on_long_task(self, Action::Pause, || self.pause_error())
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
        send_progress_on_long_task(self, Action::Delete, || self.delete())
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
        send_progress_on_long_task(self, Action::Delete, || self.delete_error())
    }

    #[named]
    fn deploy_environment(&self, environment: &Environment) -> Result<(), (HashSet<Uuid>, EngineError)> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );
        kubernetes::deploy_environment(self, environment, event_details)
    }

    #[named]
    fn pause_environment(&self, environment: &Environment) -> Result<(), (HashSet<Uuid>, EngineError)> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );
        kubernetes::pause_environment(self, environment, event_details)
    }

    #[named]
    fn delete_environment(&self, environment: &Environment) -> Result<(), (HashSet<Uuid>, EngineError)> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );
        kubernetes::delete_environment(self, environment, event_details)
    }
}
