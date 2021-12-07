mod helm_charts;
pub mod node;

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::helm::deploy_charts_levels;
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, send_progress_on_long_task, uninstall_cert_manager, Kind, Kubernetes,
    KubernetesUpgradeStatus, ProviderOptions,
};
use crate::cloud_provider::models::{NodeGroups, NodeGroupsFormat};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::scaleway::application::Zone;
use crate::cloud_provider::scaleway::kubernetes::helm_charts::{scw_helm_charts, ChartsConfigPrerequisites};
use crate::cloud_provider::scaleway::kubernetes::node::ScwInstancesType;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd::kubectl::{kubectl_exec_api_custom_metrics, kubectl_exec_get_all_namespaces, kubectl_exec_get_events};
use crate::cmd::structs::HelmChart;
use crate::cmd::terraform::{terraform_exec, terraform_init_validate_plan_apply, terraform_init_validate_state_list};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError as LegacyEngineError, EngineErrorCause, EngineErrorScope,
    SimpleError, SimpleErrorKind,
};
use crate::errors::EngineError;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
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
use std::path::PathBuf;
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
    zone: Zone,
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
        zone: Zone,
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
                        cloud_provider.kind(),
                        QoveryIdentifier::new(context.organization_id().to_string()),
                        QoveryIdentifier::new(context.cluster_id().to_string()),
                        QoveryIdentifier::new(context.execution_id().to_string()),
                        Stage::Infrastructure(InfrastructureStep::Instantiate),
                        Transmitter::Kubernetes(id, name),
                    ),
                    node_group.instance_type.as_str(),
                    e.message().to_string(),
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

    fn logs_bucket_name(&self) -> String {
        format!("qovery-logs-{}", self.id)
    }

    fn tera_context(&self) -> Result<TeraContext, LegacyEngineError> {
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
                        None => error!("VAULT_SECRET_ID environment variable wasn't found"),
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

    fn create(&self) -> Result<(), LegacyEngineError> {
        let event_details = EventDetails::new(
            self.cloud_provider.kind(),
            QoveryIdentifier::from(self.context.organization_id().to_string()),
            QoveryIdentifier::from(self.context.cluster_id().to_string()),
            QoveryIdentifier::from(self.context.execution_id().to_string()),
            Stage::Infrastructure(InfrastructureStep::Create),
            Transmitter::Kubernetes(self.id().to_string(), self.name().to_string()),
        );

        let listeners_helper = ListenersHelper::new(&self.listeners);

        // TODO(DEV-1061): remove legacy logger
        let message = format!("Preparing SCW {} cluster deployment with id {}", self.name(), self.id());
        self.send_to_customer(message.as_str(), &listeners_helper);
        self.logger.log(
            LogLevel::Info,
            EngineEvent::Deploying(event_details.clone(), EventMessage::new(message.to_string(), None)),
        );

        // upgrade cluster instead if required
        match self.config_file() {
            Ok(f) => match is_kubernetes_upgrade_required(
                f.0,
                &self.version,
                self.cloud_provider.credentials_environment_variables(),
            ) {
                Ok(x) => {
                    if x.required_upgrade_on.is_some() {
                        return self.upgrade_with_status(x);
                    }

                    self.logger.log(
                        LogLevel::Info,
                        EngineEvent::Deploying(
                            event_details.clone(),
                            EventMessage::new("Kubernetes cluster upgrade not required".to_string(), None),
                        ),
                    );
                }
                Err(e) => error!(
                    "Error detected, upgrade won't occurs, but standard deployment. {:?}",
                    e.message
                ),
            },
            Err(_) => self.logger.log(
                LogLevel::Info,
                EngineEvent::Deploying(event_details.clone(), EventMessage::new("Kubernetes cluster upgrade not required, config file is not found and cluster have certainly never been deployed before".to_string(), None)),
            ),
        };

        let temp_dir = self.get_temp_dir()?;

        // generate terraform files and copy them into temp dir
        let context = self.tera_context()?;

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                self.template_directory.as_str(),
                temp_dir.as_str(),
                &context,
            ),
        )?;

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        self.send_to_customer(
            format!("Deploying SCW {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        // terraform deployment dedicated to cloud resources
        match cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()),
        ) {
            Ok(_) => {}
            Err(e) => {
                format!(
                    "Error while deploying cluster {} with Terraform with id {}.",
                    self.name(),
                    self.id()
                );
                return Err(e);
            }
        };

        // TODO(benjaminch): move this elsewhere
        // Create object-storage buckets
        info!("Create Qovery managed object storage buckets");
        // Kubeconfig bucket
        if let Err(e) = self
            .object_storage
            .create_bucket(self.kubeconfig_bucket_name().as_str())
        {
            let message = format!(
                "Cannot create object storage bucket {} for cluster {} with id {}",
                self.kubeconfig_bucket_name(),
                self.name(),
                self.id()
            );
            error!("{}", message);
            return Err(e);
        }

        // Logs bucket
        if let Err(e) = self.object_storage.create_bucket(self.logs_bucket_name().as_str()) {
            let message = format!(
                "Cannot create object storage bucket {} for cluster {} with id {}",
                self.logs_bucket_name(),
                self.name(),
                self.id()
            );
            error!("{}", message);
            return Err(e);
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
            let message = format!(
                "Cannot put kubeconfig into object storage bucket for cluster {} with id {}",
                self.name(),
                self.id()
            );
            error!("{}. {:?}", message, e);
            return Err(e);
        }

        // kubernetes helm deployments on the cluster
        let kubeconfig = PathBuf::from(self.config_file().expect("expected to get a kubeconfig file").0);
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

        let helm_charts_to_deploy = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            scw_helm_charts(
                format!("{}/qovery-tf-config.json", &temp_dir).as_str(),
                &charts_prerequisites,
                Some(&temp_dir),
                &kubeconfig,
                &credentials_environment_variables,
            ),
        )?;

        cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            deploy_charts_levels(
                &kubeconfig,
                &credentials_environment_variables,
                helm_charts_to_deploy,
                self.context.is_dry_run_deploy(),
            ),
        )
    }

    fn create_error(&self) -> Result<(), LegacyEngineError> {
        let kubeconfig_file = match self.config_file() {
            Ok(x) => x.0,
            Err(e) => {
                error!("kubernetes cluster has just been deployed, but kubeconfig wasn't available, can't finish installation");
                return Err(e);
            }
        };
        let kubeconfig = PathBuf::from(&kubeconfig_file);
        let environment_variables: Vec<(&str, &str)> = self.cloud_provider.credentials_environment_variables();
        warn!("SCW.create_error() called for {}", self.name());
        match kubectl_exec_get_events(kubeconfig, None, environment_variables) {
            Ok(ok_line) => info!("{}", ok_line),
            Err(err) => error!("{:?}", err),
        };
        Err(self.engine_error(
            EngineErrorCause::Internal,
            format!("{} Kubernetes cluster failed on deployment", self.name()),
        ))
    }

    fn upgrade_error(&self) -> Result<(), LegacyEngineError> {
        Ok(())
    }

    fn downgrade(&self) -> Result<(), LegacyEngineError> {
        Ok(())
    }

    fn downgrade_error(&self) -> Result<(), LegacyEngineError> {
        Ok(())
    }

    fn pause(&self) -> Result<(), LegacyEngineError> {
        let listeners_helper = ListenersHelper::new(&self.listeners);
        self.send_to_customer(
            format!("Preparing SCW {} cluster pause with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        let temp_dir = self.get_temp_dir()?;

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
        let scw_ks_worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
        context.insert("scw_ks_worker_nodes", &scw_ks_worker_nodes);

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                self.template_directory.as_str(),
                temp_dir.as_str(),
                &context,
            ),
        )?;

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

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
                return Err(LegacyEngineError {
                    cause: EngineErrorCause::Internal,
                    scope: EngineErrorScope::Kubernetes(self.id.clone(), self.name.clone()),
                    execution_id: self.context.execution_id().to_string(),
                    message: e.message,
                })
            }
        };
        if tf_workers_resources.is_empty() {
            return Err(LegacyEngineError {
                cause: EngineErrorCause::Internal,
                scope: EngineErrorScope::Kubernetes(self.id.clone(), self.name.clone()),
                execution_id: self.context.execution_id().to_string(),
                message: Some("No worker nodes present, can't Pause the infrastructure. This can happen if there where a manual operations on the workers or the infrastructure is already pause.".to_string()),
            });
        }

        let kubernetes_config_file_path = self.config_file_path()?;

        // pause: wait 1h for the engine to have 0 running jobs before pausing and avoid getting unreleased lock (from helm or terraform for example)
        if self.options.qovery_engine_location == EngineLocation::ClientSide {
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
                                            error!(
                                                "error while looking at the API metric value {}. {:?}",
                                                metric_name, e
                                            );
                                            return OperationResult::Retry(SimpleError {
                                                kind: SimpleErrorKind::Other,
                                                message: Some(e.to_string()),
                                            });
                                        }
                                        _ => {}
                                    }
                                }

                                if current_engine_jobs == 0 {
                                    OperationResult::Ok(())
                                } else {
                                    OperationResult::Retry(SimpleError {
                                        kind: SimpleErrorKind::Other,
                                        message: Some("can't pause the infrastructure now, Engine jobs are currently running, retrying later...".to_string()),
                                    })
                                }
                            }
                            Err(e) => {
                                error!("error while looking at the API metric value {}. {:?}", metric_name, e);
                                OperationResult::Retry(e)
                            }
                        };
                    });

                    match wait_engine_job_finish {
                        Ok(_) => {
                            info!("no current running jobs on the Engine, infrastructure pause is allowed to start")
                        }
                        Err(Operation { error, .. }) => {
                            return Err(LegacyEngineError {
                                cause: EngineErrorCause::Internal,
                                scope: EngineErrorScope::Engine,
                                execution_id: self.context.execution_id().to_string(),
                                message: error.message,
                            })
                        }
                        Err(retry::Error::Internal(msg)) => {
                            return Err(LegacyEngineError::new(
                                EngineErrorCause::Internal,
                                EngineErrorScope::Engine,
                                self.context.execution_id(),
                                Some(msg),
                            ))
                        }
                    }
                }
                false => warn!("The Engines are running Client side, but metric history flag is disabled. You will encounter issues during cluster lifecycles if you do not enable metric history"),
            }
        }

        let mut terraform_args_string = vec!["apply".to_string(), "-auto-approve".to_string()];
        for x in tf_workers_resources {
            terraform_args_string.push(format!("-target={}", x));
        }
        let terraform_args = terraform_args_string.iter().map(|x| &**x).collect();

        let message = format!("Pausing SCW {} cluster deployment with id {}", self.name(), self.id());
        info!("{}", &message);
        self.send_to_customer(&message, &listeners_helper);

        match cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            terraform_exec(temp_dir.as_str(), terraform_args),
        ) {
            Ok(_) => {
                let message = format!("Kubernetes cluster {} successfully paused", self.name());
                info!("{}", &message);
                self.send_to_customer(&message, &listeners_helper);
                Ok(())
            }
            Err(e) => {
                error!("Error while pausing cluster {} with id {}.", self.name(), self.id());
                Err(e)
            }
        }
    }

    fn pause_error(&self) -> Result<(), LegacyEngineError> {
        Err(self.engine_error(
            EngineErrorCause::Internal,
            format!("{} Kubernetes cluster failed to pause", self.name()),
        ))
    }

    fn delete(&self) -> Result<(), LegacyEngineError> {
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let mut skip_kubernetes_step = false;
        self.send_to_customer(
            format!("Preparing to delete SCW cluster {} with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        let temp_dir = self.get_temp_dir()?;

        // generate terraform files and copy them into temp dir
        let context = self.tera_context()?;

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                self.template_directory.as_str(),
                temp_dir.as_str(),
                &context,
            ),
        )?;

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/scaleway/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        let kubernetes_config_file_path = match self.config_file_path() {
            Ok(x) => x,
            Err(e) => {
                warn!(
                    "skipping Kubernetes uninstall because it can't be reached. {:?}",
                    e.message
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
        info!("{}", &message);
        self.send_to_customer(&message, &listeners_helper);

        info!("Running Terraform apply before running a delete");
        if let Err(e) = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            cmd::terraform::terraform_init_validate_plan_apply(temp_dir.as_str(), false),
        ) {
            error!("An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy: {:?}", e.message);
        };

        if !skip_kubernetes_step {
            // should make the diff between all namespaces and qovery managed namespaces
            let message = format!(
                "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
                self.name(),
                self.id()
            );
            info!("{}", &message);
            self.send_to_customer(&message, &listeners_helper);

            let all_namespaces = kubectl_exec_get_all_namespaces(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
            );

            match all_namespaces {
                Ok(namespace_vec) => {
                    let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                    let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                    info!("Deleting non Qovery namespaces");
                    for namespace_to_delete in namespaces_to_delete.iter() {
                        info!("Starting namespace {} deletion process", namespace_to_delete);
                        let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                            &kubernetes_config_file_path,
                            namespace_to_delete,
                            self.cloud_provider().credentials_environment_variables(),
                        );

                        match deletion {
                            Ok(_) => info!("Namespace {} is deleted", namespace_to_delete),
                            Err(e) => {
                                if e.message.is_some() && e.message.unwrap().contains("not found") {
                                    {}
                                } else {
                                    error!("Can't delete the namespace {}", namespace_to_delete);
                                }
                            }
                        }
                    }
                }

                Err(e) => error!(
                    "Error while getting all namespaces for Kubernetes cluster {}: error {:?}",
                    self.name_with_id(),
                    e.message
                ),
            }

            let message = format!(
                "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
                self.name(),
                self.id()
            );
            info!("{}", &message);
            self.send_to_customer(&message, &listeners_helper);

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
            if let Err(e) = uninstall_cert_manager(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
            ) {
                return Err(LegacyEngineError::new(
                    EngineErrorCause::Internal,
                    self.engine_error_scope(),
                    self.context().execution_id(),
                    e.message,
                ));
            }

            info!("Deleting Qovery managed helm charts");
            let qovery_namespaces = get_qovery_managed_namespaces();
            for qovery_namespace in qovery_namespaces.iter() {
                info!(
                    "Starting Qovery managed charts deletion process in {} namespace",
                    qovery_namespace
                );
                let charts_to_delete = cmd::helm::helm_list(
                    &kubernetes_config_file_path,
                    self.cloud_provider().credentials_environment_variables(),
                    Some(qovery_namespace),
                );
                match charts_to_delete {
                    Ok(charts) => {
                        for chart in charts {
                            info!("Deleting chart {} in {} namespace", chart.name, chart.namespace);
                            match cmd::helm::helm_exec_uninstall(
                                &kubernetes_config_file_path,
                                &chart.namespace,
                                &chart.name,
                                self.cloud_provider().credentials_environment_variables(),
                            ) {
                                Ok(_) => info!("chart {} deleted", chart.name),
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                    Err(e) => {
                        if e.message.is_some() && e.message.unwrap().contains("not found") {
                            {}
                        } else {
                            error!("Can't delete the namespace {}", qovery_namespace);
                        }
                    }
                }
            }

            info!("Deleting Qovery managed Namespaces");
            for qovery_namespace in qovery_namespaces.iter() {
                info!("Starting namespace {} deletion process", qovery_namespace);
                let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                    &kubernetes_config_file_path,
                    qovery_namespace,
                    self.cloud_provider().credentials_environment_variables(),
                );
                match deletion {
                    Ok(_) => info!("Namespace {} is fully deleted", qovery_namespace),
                    Err(e) => {
                        if e.message.is_some() && e.message.unwrap().contains("not found") {
                            {}
                        } else {
                            error!("Can't delete the namespace {}", qovery_namespace);
                        }
                    }
                }
            }

            info!("Delete all remaining deployed helm applications");
            match cmd::helm::helm_list(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
                None,
            ) {
                Ok(helm_charts) => {
                    for chart in helm_charts {
                        info!("Deleting chart {} in progress...", chart.name);
                        let _ = cmd::helm::helm_uninstall_list(
                            &kubernetes_config_file_path,
                            vec![chart],
                            self.cloud_provider().credentials_environment_variables(),
                        );
                    }
                }
                Err(_) => error!("Unable to get helm list"),
            }
        };

        let message = format!("Deleting Kubernetes cluster {}/{}", self.name(), self.id());
        info!("{}", &message);
        self.send_to_customer(&message, &listeners_helper);

        info!("Running Terraform destroy");
        let terraform_result =
            retry::retry(
                Fibonacci::from_millis(60000).take(3),
                || match cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    cmd::terraform::terraform_init_validate_destroy(temp_dir.as_str(), false),
                ) {
                    Ok(_) => OperationResult::Ok(()),
                    Err(e) => OperationResult::Retry(e),
                },
            );

        match terraform_result {
            Ok(_) => {
                let message = format!("Kubernetes cluster {}/{} successfully deleted", self.name(), self.id());
                info!("{}", &message);
                self.send_to_customer(&message, &listeners_helper);
            }
            Err(Operation { error, .. }) => return Err(error),
            Err(retry::Error::Internal(msg)) => {
                return Err(LegacyEngineError::new(
                    EngineErrorCause::Internal,
                    self.engine_error_scope(),
                    self.context().execution_id(),
                    Some(format!(
                        "Error while deleting cluster {} with id {}: {}",
                        self.name(),
                        self.id(),
                        msg
                    )),
                ))
            }
        }

        // TODO(benjaminch): move this elsewhere
        // Delete object-storage buckets
        info!("Delete Qovery managed object storage buckets");
        if let Err(e) = self
            .object_storage
            .delete_bucket(self.kubeconfig_bucket_name().as_str())
        {
            return Err(LegacyEngineError::new(
                EngineErrorCause::Internal,
                self.engine_error_scope(),
                self.context().execution_id(),
                e.message,
            ));
        }

        if let Err(e) = self.object_storage.delete_bucket(self.logs_bucket_name().as_str()) {
            return Err(LegacyEngineError::new(
                EngineErrorCause::Internal,
                self.engine_error_scope(),
                self.context().execution_id(),
                e.message,
            ));
        }

        Ok(())
    }

    fn delete_error(&self) -> Result<(), LegacyEngineError> {
        // FIXME What should we do if something goes wrong while deleting the cluster?
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

    fn region(&self) -> &str {
        self.zone.region_str()
    }

    fn zone(&self) -> &str {
        self.zone.as_str()
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.object_storage
    }

    fn is_valid(&self) -> Result<(), LegacyEngineError> {
        Ok(())
    }

    #[named]
    fn on_create(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.create())
    }

    #[named]
    fn on_create_error(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.create_error())
    }

    fn upgrade_with_status(&self, kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), LegacyEngineError> {
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

        let temp_dir = match self.get_temp_dir() {
            Ok(dir) => dir,
            Err(e) => return Err(e),
        };

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        //
        // Upgrade nodes
        //
        let message = format!("Start upgrading process for nodes on {}/{}", self.name(), self.id());
        info!("{}", &message);
        self.send_to_customer(&message, &listeners_helper);

        context.insert(
            "kubernetes_cluster_version",
            format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
        );

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                self.template_directory.as_str(),
                temp_dir.as_str(),
                &context,
            ),
        )?;

        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        self.send_to_customer(
            format!("Upgrading Kubernetes {} nodes", self.name()).as_str(),
            &listeners_helper,
        );

        match cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()),
        ) {
            Ok(_) => {
                let message = format!("Kubernetes {} nodes have been successfully upgraded", self.name());
                info!("{}", &message);
                self.send_to_customer(&message, &listeners_helper);
            }
            Err(e) => {
                error!(
                    "Error while upgrading nodes for cluster {} with id {}.",
                    self.name(),
                    self.id()
                );
                return Err(e);
            }
        }

        Ok(())
    }

    #[named]
    fn on_upgrade(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.upgrade())
    }

    #[named]
    fn on_upgrade_error(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.upgrade_error())
    }

    #[named]
    fn on_downgrade(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.downgrade())
    }

    #[named]
    fn on_downgrade_error(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Create, || self.downgrade_error())
    }

    #[named]
    fn on_pause(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Pause, || self.pause())
    }

    #[named]
    fn on_pause_error(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Pause, || self.pause_error())
    }

    #[named]
    fn on_delete(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Delete, || self.delete())
    }

    #[named]
    fn on_delete_error(&self) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        send_progress_on_long_task(self, Action::Delete, || self.delete_error())
    }

    #[named]
    fn deploy_environment(&self, environment: &Environment) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::deploy_environment(self, environment)
    }

    #[named]
    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::deploy_environment_error(self, environment)
    }

    #[named]
    fn pause_environment(&self, _environment: &Environment) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        Ok(())
    }

    #[named]
    fn pause_environment_error(&self, _environment: &Environment) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        Ok(())
    }

    #[named]
    fn delete_environment(&self, environment: &Environment) -> Result<(), LegacyEngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::delete_environment(self, environment)
    }

    #[named]
    fn delete_environment_error(&self, _environment: &Environment) -> Result<(), LegacyEngineError> {
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
