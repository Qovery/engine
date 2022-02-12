mod helm_charts;
pub mod node;

use crate::cloud_provider::aws::regions::AwsZones;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::helm::deploy_charts_levels;
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, send_progress_on_long_task, uninstall_cert_manager, Kind, Kubernetes,
    KubernetesUpgradeStatus, ProviderOptions,
};
use crate::cloud_provider::models::{NodeGroups, NodeGroupsFormat};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::scaleway::application::ScwZone;
use crate::cloud_provider::scaleway::kubernetes::helm_charts::{scw_helm_charts, ChartsConfigPrerequisites};
use crate::cloud_provider::scaleway::kubernetes::node::ScwInstancesType;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd::kubectl::{kubectl_exec_api_custom_metrics, kubectl_exec_get_all_namespaces, kubectl_exec_get_events};
use crate::cmd::structs::HelmChart;
use crate::cmd::terraform::{terraform_exec, terraform_init_validate_plan_apply, terraform_init_validate_state_list};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
use crate::logger::{LogLevel, Logger};
use crate::models::{
    Action, Context, Features, Listen, Listener, Listeners, ListenersHelper, QoveryIdentifier, ToHelmString,
};
use crate::object_storage::scaleway_object_storage::{BucketDeleteStrategy, ScalewayOS};
use crate::object_storage::ObjectStorage;
use crate::string::terraform_list_format;
use crate::{cmd, dns_provider};
use ::function_name::named;
use retry::delay::{Fibonacci, Fixed};
use retry::Error::Operation;
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use std::str::FromStr;
use tera::Context as TeraContext;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KapsuleOptions {
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    pub qovery_cluster_secret_token: String,
    pub qovery_nats_url: String,
    pub qovery_nats_user: String,
    pub qovery_nats_password: String,
    pub qovery_ssh_key: String,
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
        qovery_cluster_secret_token: String,
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
            qovery_cluster_secret_token,
            qovery_nats_url,
            qovery_nats_user,
            qovery_nats_password,
            qovery_ssh_key,
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

pub struct Kapsule<'a> {
    context: Context,
    id: String,
    long_id: uuid::Uuid,
    name: String,
    version: String,
    zone: ScwZone,
    cloud_provider: &'a dyn CloudProvider,
    dns_provider: &'a dyn DnsProvider,
    object_storage: ScalewayOS,
    nodes_groups: Vec<NodeGroups>,
    template_directory: String,
    options: KapsuleOptions,
    listeners: Listeners,
    logger: &'a dyn Logger,
}

impl<'a> Kapsule<'a> {
    pub fn new(
        context: Context,
        id: String,
        long_id: uuid::Uuid,
        name: String,
        version: String,
        zone: ScwZone,
        cloud_provider: &'a dyn CloudProvider,
        dns_provider: &'a dyn DnsProvider,
        nodes_groups: Vec<NodeGroups>,
        options: KapsuleOptions,
        logger: &'a dyn Logger,
    ) -> Result<Kapsule<'a>, EngineError> {
        let template_directory = format!("{}/scaleway/bootstrap", context.lib_root_dir());

        for node_group in &nodes_groups {
            if let Err(e) = ScwInstancesType::from_str(node_group.instance_type.as_str()) {
                let err = EngineError::new_unsupported_instance_type(
                    EventDetails::new(
                        Some(cloud_provider.kind()),
                        QoveryIdentifier::new(context.organization_id().to_string()),
                        QoveryIdentifier::new(context.cluster_id().to_string()),
                        QoveryIdentifier::new(context.execution_id().to_string()),
                        Some(zone.region_str().to_string()),
                        Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
                        Transmitter::Kubernetes(id, name),
                    ),
                    node_group.instance_type.as_str(),
                    e,
                );

                logger.log(LogLevel::Error, EngineEvent::Error(err.clone()));

                return Err(err);
            }
        }

        let object_storage = ScalewayOS::new(
            context.clone(),
            "s3-temp-id".to_string(),
            "default-s3".to_string(),
            cloud_provider.access_key_id().clone(),
            cloud_provider.secret_access_key().clone(),
            zone,
            BucketDeleteStrategy::Empty,
            false,
            context.resource_expiration_in_seconds(),
        );

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
            listeners: cloud_provider.listeners().clone(), // copy listeners from CloudProvider
        })
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::LoadConfiguration));
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
        context.insert(
            "managed_dns_domains_root_helm_format",
            &managed_dns_domains_root_helm_format,
        );
        context.insert(
            "managed_dns_domains_terraform_format",
            &managed_dns_domains_terraform_format,
        );
        context.insert(
            "managed_dns_domains_root_terraform_format",
            &managed_dns_domains_root_terraform_format,
        );
        context.insert(
            "managed_dns_resolvers_terraform_format",
            &managed_dns_resolvers_terraform_format,
        );
        match self.dns_provider.kind() {
            dns_provider::Kind::Cloudflare => {
                context.insert("external_dns_provider", self.dns_provider.provider_name());
                context.insert("cloudflare_api_token", self.dns_provider.token());
                context.insert("cloudflare_email", self.dns_provider.account());
            }
        };

        context.insert("dns_email_report", &self.options.tls_email_report);

        // Kubernetes
        context.insert("test_cluster", &self.context.is_test_cluster());
        context.insert("kubernetes_cluster_id", self.id());
        context.insert("kubernetes_cluster_name", self.name());
        context.insert("kubernetes_cluster_version", self.version());

        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("object_storage_kubeconfig_bucket", &self.kubeconfig_bucket_name());
        context.insert("object_storage_logs_bucket", &self.logs_bucket_name());

        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());
        context.insert("qovery_nats_url", self.options.qovery_nats_url.as_str());
        context.insert("qovery_nats_user", self.options.qovery_nats_user.as_str());
        context.insert("qovery_nats_password", self.options.qovery_nats_password.as_str());
        context.insert(
            "engine_version_controller_token",
            &self.options.engine_version_controller_token,
        );
        context.insert(
            "agent_version_controller_token",
            &self.options.agent_version_controller_token,
        );

        // Qovery features
        context.insert(
            "log_history_enabled",
            &self.context.is_feature_enabled(&Features::LogsHistory),
        );
        context.insert(
            "metrics_history_enabled",
            &self.context.is_feature_enabled(&Features::MetricsHistory),
        );
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
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
                        None => self.logger().log(
                            LogLevel::Error,
                            EngineEvent::Error(EngineError::new_missing_required_env_variable(
                                event_details.clone(),
                                "VAULT_SECRET_ID".to_string(),
                            )),
                        ),
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));

        // TODO(DEV-1061): remove legacy logger
        self.send_to_customer(
            format!("Preparing SCW {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger.log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Preparing SCW cluster deployment.".to_string()),
            ),
        );

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

                    self.logger().log(
                        LogLevel::Info,
                        EngineEvent::Deploying(
                            event_details.clone(),
                            EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                        ),
                    )
                }
                Err(e) => {
                    self.logger().log(LogLevel::Error, EngineEvent::Error(e));
                    self.logger().log(
                        LogLevel::Info,
                        EngineEvent::Deploying(
                            event_details.clone(),
                            EventMessage::new_from_safe(
                                "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                            ),
                        ),
                    );
                }
            },
            Err(_) => self.logger().log(LogLevel::Info, EngineEvent::Deploying(event_details.clone(), EventMessage::new_from_safe("Kubernetes cluster upgrade not required, config file is not found and cluster have certainly never been deployed before".to_string())))

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
                event_details.clone(),
                self.template_directory.to_string(),
                temp_dir.to_string(),
                e,
            ));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        if let Err(e) =
            crate::template::copy_non_template_files(bootstrap_charts_dir.to_string(), common_charts_temp_dir.as_str())
        {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details.clone(),
                bootstrap_charts_dir.to_string(),
                common_charts_temp_dir.to_string(),
                e,
            ));
        }

        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Deploying SCW cluster.".to_string()),
            ),
        );

        self.send_to_customer(
            format!("Deploying SCW {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        // temporary: remove helm/kube management from terraform
        match terraform_init_validate_state_list(temp_dir.as_str()) {
            Ok(x) => {
                let items_type = vec!["helm_release", "kubernetes_namespace"];
                for item in items_type {
                    for entry in x.clone() {
                        if entry.starts_with(item) {
                            match terraform_exec(temp_dir.as_str(), vec!["state", "rm", &entry]) {
                                Ok(_) => self.logger().log(
                                    LogLevel::Info,
                                    EngineEvent::Deploying(
                                        event_details.clone(),
                                        EventMessage::new_from_safe(format!("successfully removed {}", &entry)),
                                    ),
                                ),
                                Err(e) => {
                                    return Err(EngineError::new_terraform_cannot_remove_entry_out(
                                        event_details.clone(),
                                        entry.to_string(),
                                        e,
                                    ))
                                }
                            }
                        };
                    }
                }
            }
            Err(e) => self.logger().log(
                LogLevel::Warning,
                EngineEvent::Error(EngineError::new_terraform_state_does_not_exist(
                    event_details.clone(),
                    e,
                )),
            ),
        };

        // TODO(benjaminch): move this elsewhere
        // Create object-storage buckets
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Create Qovery managed object storage buckets".to_string()),
            ),
        );
        if let Err(e) = self
            .object_storage
            .create_bucket(self.kubeconfig_bucket_name().as_str())
        {
            let error = EngineError::new_object_storage_cannot_create_bucket_error(
                event_details.clone(),
                self.kubeconfig_bucket_name(),
                CommandError::new(e.message.unwrap_or("No error message".to_string()), None),
            );
            self.logger().log(LogLevel::Error, EngineEvent::Error(error.clone()));
            return Err(error);
        }

        // Logs bucket
        if let Err(e) = self.object_storage.create_bucket(self.logs_bucket_name().as_str()) {
            let error = EngineError::new_object_storage_cannot_create_bucket_error(
                event_details.clone(),
                self.logs_bucket_name(),
                CommandError::new(e.message.unwrap_or("No error message".to_string()), None),
            );
            self.logger().log(LogLevel::Error, EngineEvent::Error(error.clone()));
            return Err(error);
        }

        // terraform deployment dedicated to cloud resources
        if let Err(e) = terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            return Err(EngineError::new_terraform_error_while_executing_pipeline(
                event_details.clone(),
                e,
            ));
        }

        // push config file to object storage
        let kubeconfig_name = format!("{}.yaml", self.id());
        if let Err(e) = self.object_storage.put(
            self.kubeconfig_bucket_name().as_str(),
            kubeconfig_name.as_str(),
            format!(
                "{}/{}/{}",
                temp_dir.as_str(),
                self.kubeconfig_bucket_name().as_str(),
                kubeconfig_name.as_str()
            )
            .as_str(),
        ) {
            let error = EngineError::new_object_storage_cannot_put_file_into_bucket_error(
                event_details.clone(),
                self.logs_bucket_name(),
                kubeconfig_name.to_string(),
                CommandError::new(e.message.unwrap_or("No error message".to_string()), None),
            );
            self.logger().log(LogLevel::Error, EngineEvent::Error(error.clone()));
            return Err(error);
        }

        match self.check_workers_on_create() {
            Ok(_) => {
                self.send_to_customer(
                    format!("Kubernetes {} nodes have been successfully created", self.name()).as_str(),
                    &listeners_helper,
                );
                self.logger().log(
                    LogLevel::Info,
                    EngineEvent::Deploying(
                        event_details.clone(),
                        EventMessage::new_from_safe("Kubernetes nodes have been successfully created".to_string()),
                    ),
                )
            }
            Err(e) => {
                return Err(EngineError::new_k8s_node_not_ready(event_details.clone(), e));
            }
        };

        // kubernetes helm deployments on the cluster
        let kubeconfig_path = &self.get_kubeconfig_file_path()?;
        let kubeconfig_path = Path::new(kubeconfig_path);

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
            self.cloud_provider.access_key_id().to_string(),
            self.cloud_provider.secret_access_key().to_string(),
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
            self.dns_provider.account().to_string(),
            self.dns_provider.token().to_string(),
            self.context.disable_pleco(),
            self.options.clone(),
        );

        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
            ),
        );
        let helm_charts_to_deploy = scw_helm_charts(
            format!("{}/qovery-tf-config.json", &temp_dir).as_str(),
            &charts_prerequisites,
            Some(&temp_dir),
            &kubeconfig_path,
            &credentials_environment_variables,
        )
        .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?;

        deploy_charts_levels(
            &kubeconfig_path,
            &credentials_environment_variables,
            helm_charts_to_deploy,
            self.context.is_dry_run_deploy(),
        )
        .map_err(|e| EngineError::new_helm_charts_deploy_error(event_details.clone(), e))
    }

    fn create_error(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
        let (kubeconfig_path, _) = self.get_kubeconfig_file()?;
        let environment_variables: Vec<(&str, &str)> = self.cloud_provider.credentials_environment_variables();

        self.logger().log(
            LogLevel::Warning,
            EngineEvent::Deploying(
                self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create)),
                EventMessage::new_from_safe("SCW.create_error() called.".to_string()),
            ),
        );

        match kubectl_exec_get_events(kubeconfig_path, None, environment_variables) {
            Ok(ok_line) => self.logger().log(
                LogLevel::Info,
                EngineEvent::Deploying(event_details.clone(), EventMessage::new_from_safe(ok_line)),
            ),
            Err(err) => self.logger().log(
                LogLevel::Error,
                EngineEvent::Deploying(
                    event_details.clone(),
                    EventMessage::new("Error trying to get kubernetes events".to_string(), Some(err.message())),
                ),
            ),
        };

        Ok(())
    }

    fn upgrade_error(&self) -> Result<(), EngineError> {
        self.logger().log(
            LogLevel::Warning,
            EngineEvent::Deploying(
                self.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade)),
                EventMessage::new_from_safe("SCW.upgrade_error() called.".to_string()),
            ),
        );

        Ok(())
    }

    fn downgrade(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn downgrade_error(&self) -> Result<(), EngineError> {
        self.logger().log(
            LogLevel::Warning,
            EngineEvent::Deploying(
                self.get_event_details(Stage::Infrastructure(InfrastructureStep::Downgrade)),
                EventMessage::new_from_safe("SCW.downgrade_error() called.".to_string()),
            ),
        );

        Ok(())
    }

    fn pause(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));
        let listeners_helper = ListenersHelper::new(&self.listeners);

        self.send_to_customer(
            format!("Preparing SCW {} cluster pause with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        self.logger().log(
            LogLevel::Info,
            EngineEvent::Pausing(
                self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
                EventMessage::new_from_safe("Preparing SCW cluster pause.".to_string()),
            ),
        );

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
                event_details.clone(),
                self.template_directory.to_string(),
                temp_dir.to_string(),
                e,
            ));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        if let Err(e) =
            crate::template::copy_non_template_files(bootstrap_charts_dir.to_string(), common_charts_temp_dir.as_str())
        {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details.clone(),
                bootstrap_charts_dir.to_string(),
                common_charts_temp_dir.to_string(),
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
                let error = EngineError::new_terraform_state_does_not_exist(event_details.clone(), e);
                self.logger().log(LogLevel::Error, EngineEvent::Error(error.clone()));
                return Err(error);
            }
        };

        if tf_workers_resources.is_empty() {
            return Err(EngineError::new_cluster_has_no_worker_nodes(event_details.clone()));
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
                                            let safe_message = "Error while looking at the API metric value";
                                            return OperationResult::Retry(EngineError::new_cannot_get_k8s_api_custom_metrics(event_details.clone(), CommandError::new(format!("{}, error: {}", safe_message, e.to_string()), Some(safe_message.to_string()))));
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
                                let safe_message = format!("Error while looking at the API metric value {}", metric_name);
                                OperationResult::Retry(
                                    EngineError::new_cannot_get_k8s_api_custom_metrics(event_details.clone(), CommandError::new(format!("{}, error: {}", safe_message, e.message()), Some(safe_message.to_string()))))
                            }
                        };
                    });

                    match wait_engine_job_finish {
                        Ok(_) => {
                            self.logger().log(LogLevel::Info, EngineEvent::Pausing(event_details.clone(), EventMessage::new_from_safe("No current running jobs on the Engine, infrastructure pause is allowed to start".to_string())));
                        }
                        Err(Operation { error, .. }) => {
                            return Err(error)
                        }
                        Err(retry::Error::Internal(msg)) => {
                            return Err(EngineError::new_cannot_pause_cluster_tasks_are_running(event_details.clone(), Some(CommandError::new_from_safe_message(msg))))
                        }
                    }
                }
                false => self.logger().log(LogLevel::Warning, EngineEvent::Pausing(event_details.clone(), EventMessage::new_from_safe("The Engines are running Client side, but metric history flag is disabled. You will encounter issues during cluster lifecycles if you do not enable metric history".to_string()))),
            }
        }

        let mut terraform_args_string = vec!["apply".to_string(), "-auto-approve".to_string()];
        for x in tf_workers_resources {
            terraform_args_string.push(format!("-target={}", x));
        }
        let terraform_args = terraform_args_string.iter().map(|x| &**x).collect();

        self.send_to_customer(
            format!("Pausing SCW {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Pausing(
                event_details.clone(),
                EventMessage::new_from_safe("Pausing SCW cluster deployment.".to_string()),
            ),
        );

        match terraform_exec(temp_dir.as_str(), terraform_args) {
            Ok(_) => {
                let message = format!("Kubernetes cluster {} successfully paused", self.name());
                self.send_to_customer(&message, &listeners_helper);
                self.logger().log(
                    LogLevel::Info,
                    EngineEvent::Pausing(event_details.clone(), EventMessage::new_from_safe(message)),
                );
                Ok(())
            }
            Err(e) => Err(EngineError::new_terraform_error_while_executing_pipeline(
                event_details.clone(),
                e,
            )),
        }
    }

    fn pause_error(&self) -> Result<(), EngineError> {
        self.logger().log(
            LogLevel::Warning,
            EngineEvent::Pausing(
                self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
                EventMessage::new_from_safe("SCW.pause_error() called.".to_string()),
            ),
        );

        Ok(())
    }

    fn delete(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let mut skip_kubernetes_step = false;

        self.send_to_customer(
            format!("Preparing to delete SCW cluster {} with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger().log(
            LogLevel::Warning,
            EngineEvent::Deleting(
                event_details.clone(),
                EventMessage::new_from_safe("Preparing to delete SCW cluster.".to_string()),
            ),
        );

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let context = self.tera_context()?;

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            context,
        ) {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details.clone(),
                self.template_directory.to_string(),
                temp_dir.to_string(),
                e,
            ));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        if let Err(e) =
            crate::template::copy_non_template_files(bootstrap_charts_dir.to_string(), common_charts_temp_dir.as_str())
        {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details.clone(),
                bootstrap_charts_dir.to_string(),
                common_charts_temp_dir.to_string(),
                e,
            ));
        }

        let kubernetes_config_file_path = match self.get_kubeconfig_file_path() {
            Ok(x) => x,
            Err(e) => {
                let safe_message = "Skipping Kubernetes uninstall because it can't be reached.";
                self.logger().log(
                    LogLevel::Warning,
                    EngineEvent::Deleting(
                        event_details.clone(),
                        EventMessage::new(safe_message.to_string(), Some(e.message())),
                    ),
                );
                skip_kubernetes_step = true;
                "".to_string()
            }
        };

        // should apply before destroy to be sure destroy will compute on all resources
        // don't exit on failure, it can happen if we resume a destroy process
        let message = format!(
            "Ensuring everything is up to date before deleting cluster {}/{}",
            self.name(),
            self.id()
        );
        self.send_to_customer(&message, &listeners_helper);
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deleting(event_details.clone(), EventMessage::new_from_safe(message)),
        );

        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deleting(
                event_details.clone(),
                EventMessage::new_from_safe("Running Terraform apply before running a delete.".to_string()),
            ),
        );
        if let Err(e) = cmd::terraform::terraform_init_validate_plan_apply(temp_dir.as_str(), false) {
            // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
            self.logger().log(
                LogLevel::Error,
                EngineEvent::Error(EngineError::new_terraform_error_while_executing_pipeline(
                    event_details.clone(),
                    e,
                )),
            );
        };

        if !skip_kubernetes_step {
            // should make the diff between all namespaces and qovery managed namespaces
            let message = format!(
                "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
                self.name(),
                self.id()
            );
            self.logger().log(
                LogLevel::Info,
                EngineEvent::Deleting(event_details.clone(), EventMessage::new_from_safe(message.to_string())),
            );
            self.send_to_customer(&message, &listeners_helper);

            let all_namespaces = kubectl_exec_get_all_namespaces(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
            );

            match all_namespaces {
                Ok(namespace_vec) => {
                    let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                    let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                    self.logger().log(
                        LogLevel::Info,
                        EngineEvent::Deleting(
                            event_details.clone(),
                            EventMessage::new_from_safe("Deleting non Qovery namespaces".to_string()),
                        ),
                    );

                    for namespace_to_delete in namespaces_to_delete.iter() {
                        match cmd::kubectl::kubectl_exec_delete_namespace(
                            &kubernetes_config_file_path,
                            namespace_to_delete,
                            self.cloud_provider().credentials_environment_variables(),
                        ) {
                            Ok(_) => self.logger().log(
                                LogLevel::Info,
                                EngineEvent::Deleting(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(format!(
                                        "Namespace `{}` deleted successfully.",
                                        namespace_to_delete
                                    )),
                                ),
                            ),
                            Err(e) => {
                                if !(e.message().contains("not found")) {
                                    self.logger().log(
                                        LogLevel::Error,
                                        EngineEvent::Deleting(
                                            event_details.clone(),
                                            EventMessage::new_from_safe(format!(
                                                "Can't delete the namespace `{}`",
                                                namespace_to_delete
                                            )),
                                        ),
                                    );
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
                    self.logger().log(
                        LogLevel::Error,
                        EngineEvent::Deleting(
                            event_details.clone(),
                            EventMessage::new(message_safe, Some(e.message())),
                        ),
                    );
                }
            }

            let message = format!(
                "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
                self.name(),
                self.id()
            );
            self.send_to_customer(&message, &listeners_helper);
            self.logger().log(
                LogLevel::Info,
                EngineEvent::Deleting(event_details.clone(), EventMessage::new_from_safe(message)),
            );

            // delete custom metrics api to avoid stale namespaces on deletion
            let _ = cmd::helm::helm_uninstall_list(
                &kubernetes_config_file_path,
                vec![HelmChart {
                    name: "metrics-server".to_string(),
                    namespace: "kube-system".to_string(),
                    version: None,
                }],
                self.cloud_provider().credentials_environment_variables(),
            );

            // required to avoid namespace stuck on deletion
            uninstall_cert_manager(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
                event_details.clone(),
                self.logger(),
            )?;

            self.logger().log(
                LogLevel::Info,
                EngineEvent::Deleting(
                    event_details.clone(),
                    EventMessage::new_from_safe("Deleting Qovery managed helm charts".to_string()),
                ),
            );

            let qovery_namespaces = get_qovery_managed_namespaces();
            for qovery_namespace in qovery_namespaces.iter() {
                let charts_to_delete = cmd::helm::helm_list(
                    &kubernetes_config_file_path,
                    self.cloud_provider().credentials_environment_variables(),
                    Some(qovery_namespace),
                );
                match charts_to_delete {
                    Ok(charts) => {
                        for chart in charts {
                            match cmd::helm::helm_exec_uninstall(
                                &kubernetes_config_file_path,
                                &chart.namespace,
                                &chart.name,
                                self.cloud_provider().credentials_environment_variables(),
                            ) {
                                Ok(_) => self.logger().log(
                                    LogLevel::Info,
                                    EngineEvent::Deleting(
                                        event_details.clone(),
                                        EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                                    ),
                                ),
                                Err(e) => {
                                    let message_safe = format!("Can't delete chart `{}`", chart.name);
                                    self.logger().log(
                                        LogLevel::Error,
                                        EngineEvent::Deleting(
                                            event_details.clone(),
                                            EventMessage::new(message_safe, Some(e.message())),
                                        ),
                                    )
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if !(e.message().contains("not found")) {
                            self.logger().log(
                                LogLevel::Error,
                                EngineEvent::Deleting(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(format!(
                                        "Can't delete the namespace {}",
                                        qovery_namespace
                                    )),
                                ),
                            )
                        }
                    }
                }
            }

            self.logger().log(
                LogLevel::Info,
                EngineEvent::Deleting(
                    event_details.clone(),
                    EventMessage::new_from_safe("Deleting Qovery managed namespaces".to_string()),
                ),
            );

            for qovery_namespace in qovery_namespaces.iter() {
                let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                    &kubernetes_config_file_path,
                    qovery_namespace,
                    self.cloud_provider().credentials_environment_variables(),
                );
                match deletion {
                    Ok(_) => self.logger().log(
                        LogLevel::Info,
                        EngineEvent::Deleting(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Namespace {} is fully deleted", qovery_namespace)),
                        ),
                    ),
                    Err(e) => {
                        if !(e.message().contains("not found")) {
                            self.logger().log(
                                LogLevel::Error,
                                EngineEvent::Deleting(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(format!(
                                        "Can't delete namespace {}.",
                                        qovery_namespace
                                    )),
                                ),
                            )
                        }
                    }
                }
            }

            self.logger().log(
                LogLevel::Info,
                EngineEvent::Deleting(
                    event_details.clone(),
                    EventMessage::new_from_safe("Delete all remaining deployed helm applications".to_string()),
                ),
            );

            match cmd::helm::helm_list(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
                None,
            ) {
                Ok(helm_charts) => {
                    for chart in helm_charts {
                        match cmd::helm::helm_uninstall_list(
                            &kubernetes_config_file_path,
                            vec![chart.clone()],
                            self.cloud_provider().credentials_environment_variables(),
                        ) {
                            Ok(_) => self.logger().log(
                                LogLevel::Info,
                                EngineEvent::Deleting(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                                ),
                            ),
                            Err(e) => {
                                let message_safe = format!("Error deleting chart `{}` deleted", chart.name);
                                self.logger().log(
                                    LogLevel::Error,
                                    EngineEvent::Deleting(
                                        event_details.clone(),
                                        EventMessage::new(message_safe, e.message),
                                    ),
                                )
                            }
                        }
                    }
                }
                Err(e) => {
                    let message_safe = "Unable to get helm list";
                    self.logger().log(
                        LogLevel::Error,
                        EngineEvent::Deleting(
                            event_details.clone(),
                            EventMessage::new(message_safe.to_string(), Some(e.message())),
                        ),
                    )
                }
            }
        };

        let message = format!("Deleting Kubernetes cluster {}/{}", self.name(), self.id());
        self.send_to_customer(&message, &listeners_helper);
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deleting(event_details.clone(), EventMessage::new_from_safe(message)),
        );

        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deleting(
                event_details.clone(),
                EventMessage::new_from_safe("Running Terraform destroy".to_string()),
            ),
        );

        match retry::retry(Fibonacci::from_millis(60000).take(3), || {
            match cmd::terraform::terraform_init_validate_destroy(temp_dir.as_str(), false) {
                Ok(_) => OperationResult::Ok(()),
                Err(e) => OperationResult::Retry(e),
            }
        }) {
            Ok(_) => {
                self.send_to_customer(
                    format!("Kubernetes cluster {}/{} successfully deleted", self.name(), self.id()).as_str(),
                    &listeners_helper,
                );
                self.logger().log(
                    LogLevel::Info,
                    EngineEvent::Deleting(
                        event_details.clone(),
                        EventMessage::new_from_safe("Kubernetes cluster successfully deleted".to_string()),
                    ),
                );
                Ok(())
            }
            Err(Operation { error, .. }) => Err(EngineError::new_terraform_error_while_executing_destroy_pipeline(
                event_details.clone(),
                error,
            )),
            Err(retry::Error::Internal(msg)) => Err(EngineError::new_terraform_error_while_executing_destroy_pipeline(
                event_details.clone(),
                CommandError::new(msg, None),
            )),
        }
    }

    fn delete_error(&self) -> Result<(), EngineError> {
        self.logger().log(
            LogLevel::Warning,
            EngineEvent::Deleting(
                self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete)),
                EventMessage::new_from_safe("SCW.delete_error() called.".to_string()),
            ),
        );

        Ok(())
    }

    fn cloud_provider_name(&self) -> &str {
        "scaleway"
    }

    fn struct_name(&self) -> &str {
        "kubernetes"
    }
}

impl<'a> Kubernetes for Kapsule<'a> {
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

    fn region(&self) -> String {
        self.zone.region_str()
    }

    fn zone(&self) -> &str {
        self.zone.as_str()
    }

    fn aws_zones(&self) -> Option<Vec<AwsZones>> {
        None
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider
    }

    fn logger(&self) -> &dyn Logger {
        self.logger
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.object_storage
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_create(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.create())
    }

    #[named]
    fn on_create_error(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
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
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Start preparing SCW cluster upgrade process".to_string()),
            ),
        );

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        if let Err(e) = self.delete_crashlooping_pods(
            None,
            None,
            Some(3),
            self.cloud_provider().credentials_environment_variables(),
            Stage::Infrastructure(InfrastructureStep::Upgrade),
        ) {
            self.logger().log(LogLevel::Error, EngineEvent::Error(e.clone()));
            return Err(e);
        }

        //
        // Upgrade nodes
        //
        self.send_to_customer(
            format!("Preparing nodes for upgrade for Kubernetes cluster {}", self.name()).as_str(),
            &listeners_helper,
        );
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Preparing nodes for upgrade for Kubernetes cluster.".to_string()),
            ),
        );

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
                event_details.clone(),
                self.template_directory.to_string(),
                temp_dir.to_string(),
                e,
            ));
        }

        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let common_bootstrap_charts = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        if let Err(e) =
            crate::template::copy_non_template_files(common_bootstrap_charts.as_str(), common_charts_temp_dir.as_str())
        {
            return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details.clone(),
                common_bootstrap_charts.to_string(),
                common_charts_temp_dir.to_string(),
                e,
            ));
        }

        self.send_to_customer(
            format!("Upgrading Kubernetes {} nodes", self.name()).as_str(),
            &listeners_helper,
        );
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Upgrading Kubernetes nodes.".to_string()),
            ),
        );

        match terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            Ok(_) => match self.check_workers_on_upgrade(kubernetes_upgrade_status.requested_version.to_string()) {
                Ok(_) => {
                    self.send_to_customer(
                        format!("Kubernetes {} nodes have been successfully upgraded", self.name()).as_str(),
                        &listeners_helper,
                    );
                    self.logger().log(
                        LogLevel::Info,
                        EngineEvent::Deploying(
                            event_details.clone(),
                            EventMessage::new_from_safe(
                                "Kubernetes nodes have been successfully upgraded.".to_string(),
                            ),
                        ),
                    );
                }
                Err(e) => {
                    return Err(EngineError::new_k8s_node_not_ready_with_requested_version(
                        event_details.clone(),
                        kubernetes_upgrade_status.requested_version.to_string(),
                        e,
                    ));
                }
            },
            Err(e) => {
                return Err(EngineError::new_terraform_error_while_executing_pipeline(
                    event_details.clone(),
                    e,
                ));
            }
        }

        Ok(())
    }

    #[named]
    fn on_upgrade(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.upgrade())
    }

    #[named]
    fn on_upgrade_error(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.upgrade_error())
    }

    #[named]
    fn on_downgrade(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.downgrade())
    }

    #[named]
    fn on_downgrade_error(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.downgrade_error())
    }

    #[named]
    fn on_pause(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Pause, || self.pause())
    }

    #[named]
    fn on_pause_error(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Pause, || self.pause_error())
    }

    #[named]
    fn on_delete(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Delete, || self.delete())
    }

    #[named]
    fn on_delete_error(&self) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Delete, || self.delete_error())
    }

    #[named]
    fn deploy_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::deploy_environment(self, environment, event_details)
    }

    #[named]
    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::deploy_environment_error(self, environment, event_details)
    }

    #[named]
    fn pause_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::pause_environment(self, environment, event_details)
    }

    #[named]
    fn pause_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
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
        );
        kubernetes::delete_environment(self, environment, event_details)
    }

    #[named]
    fn delete_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        Ok(())
    }
}

impl<'a> Listen for Kapsule<'a> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
