pub mod helm_charts;
pub mod io;

use crate::cloud_provider::gcp::locations::GcpRegion;
use crate::cloud_provider::helm::{deploy_charts_levels, ChartInfo};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, send_progress_on_long_task, uninstall_cert_manager, Kind, Kubernetes,
    KubernetesUpgradeStatus, KubernetesVersion, ProviderOptions,
};
use crate::cloud_provider::models::CpuArchitecture;
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsGcp};
use crate::cloud_provider::CloudProvider;
use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::cmd::helm::Helm;
use crate::cmd::kubectl::{kubectl_exec_delete_namespace, kubectl_exec_get_all_namespaces, kubectl_exec_get_events};
use crate::cmd::terraform::{terraform_init_validate_destroy, terraform_init_validate_plan_apply, TerraformError};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
use crate::io_models::context::{Context, Features};
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::QoveryIdentifier;
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use crate::models::domain::ToHelmString;
use crate::models::gcp::JsonCredentials;
use crate::models::third_parties::LetsEncryptConfig;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::errors::ObjectStorageError;
use crate::object_storage::google_object_storage::GoogleOS;
use crate::object_storage::{BucketDeleteStrategy, ObjectStorage};
use crate::secret_manager;
use crate::secret_manager::vault::QVaultClient;
use crate::services::gcp::object_storage_regions::GcpStorageRegion;
use crate::services::gcp::object_storage_service::ObjectStorageService;
use crate::string::terraform_list_format;
use base64::engine::general_purpose;
use base64::Engine;
use function_name::named;
use governor::{Quota, RateLimiter};
use itertools::Itertools;
use nonzero_ext::nonzero;
use serde_derive::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::{env, fs};
use tera::Context as TeraContext;
use time::{format_description, Time};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VpcMode {
    Automatic,
    ExistingVpc { vpc_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GkeOptions {
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    pub qovery_engine_url: String,
    pub jwt_token: String,
    pub qovery_ssh_key: String,
    pub user_ssh_keys: Vec<String>,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub qovery_engine_location: EngineLocation,

    // GCP
    pub gcp_json_credentials: JsonCredentials,

    // VPC
    pub vpc_mode: VpcMode,

    // GCP to be checked during integration if needed:
    pub cluster_maintenance_start_time: Time,
    pub cluster_maintenance_end_time: Option<Time>,

    // Other
    pub tls_email_report: String,
}

impl GkeOptions {
    pub fn new(
        qovery_api_url: String,
        qovery_grpc_url: String,
        qovery_engine_url: String,
        jwt_token: String,
        qovery_ssh_key: String,
        user_ssh_keys: Vec<String>,
        grafana_admin_user: String,
        grafana_admin_password: String,
        qovery_engine_location: EngineLocation,
        gcp_json_credentials: JsonCredentials,
        vpc_mode: VpcMode,
        tls_email_report: String,
        cluster_maintenance_start_time: Time,
        cluster_maintenance_end_time: Option<Time>,
    ) -> Self {
        GkeOptions {
            qovery_api_url,
            qovery_grpc_url,
            qovery_engine_url,
            jwt_token,
            qovery_ssh_key,
            user_ssh_keys,
            grafana_admin_user,
            grafana_admin_password,
            qovery_engine_location,
            gcp_json_credentials,
            vpc_mode,
            tls_email_report,
            cluster_maintenance_start_time,
            cluster_maintenance_end_time,
        }
    }
}

impl ProviderOptions for GkeOptions {}

pub struct Gke {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    version: KubernetesVersion,
    region: GcpRegion,
    template_directory: String,
    cloud_provider: Arc<dyn CloudProvider>,
    dns_provider: Arc<dyn DnsProvider>,
    object_storage: GoogleOS,
    options: GkeOptions,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    advanced_settings: ClusterAdvancedSettings,
    customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
}

impl Gke {
    pub fn new(
        context: Context,
        id: &str,
        long_id: Uuid,
        name: &str,
        version: KubernetesVersion,
        region: GcpRegion,
        cloud_provider: Arc<dyn CloudProvider>,
        dns_provider: Arc<dyn DnsProvider>,
        options: GkeOptions,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    ) -> Result<Self, Box<EngineError>> {
        let event_details = EventDetails::new(
            Some(cloud_provider.kind()),
            QoveryIdentifier::new(*context.organization_long_id()),
            QoveryIdentifier::new(*context.cluster_long_id()),
            context.borrow().execution_id().to_string(),
            Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::Kubernetes(long_id, name.to_string()),
        );

        let google_object_storage = GoogleOS::new(
            id,
            long_id,
            name,
            &options.gcp_json_credentials.project_id.to_string(),
            GcpStorageRegion::from(region.clone()),
            Arc::new(
                ObjectStorageService::new(
                    options.gcp_json_credentials.clone(),
                    // A rate limiter making sure to keep the QPS under quotas while bucket writes requests
                    // Max default quotas are 0.5 RPS
                    // more info here https://cloud.google.com/storage/quotas?hl=fr
                    Some(Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(30_u32))))),
                    // A rate limiter making sure to keep the QPS under quotas while bucket objects writes requests
                    // Max default quotas are 1 RPS
                    // more info here https://cloud.google.com/storage/quotas?hl=fr
                    Some(Arc::from(RateLimiter::direct(Quota::per_second(nonzero!(1_u32))))),
                )
                .map_err(|e| {
                    Box::new(EngineError::new_object_storage_error(
                        event_details,
                        ObjectStorageError::CannotInstantiateClient {
                            raw_error_message: e.to_string(),
                        },
                    ))
                })?,
            ),
        );

        Ok(Self {
            context: context.clone(),
            id: id.to_string(),
            long_id,
            name: name.to_string(),
            version,
            region,
            template_directory: format!("{}/gcp/bootstrap", context.lib_root_dir()),
            cloud_provider,
            dns_provider,
            object_storage: google_object_storage,
            options,
            logger,
            metrics_registry,
            advanced_settings,
            customer_helm_charts_override,
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

    fn tera_context(&self) -> Result<TeraContext, Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        let mut context = TeraContext::new();

        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("organization_long_id", &self.cloud_provider.organization_long_id().to_string());
        context.insert("object_storage_kubeconfig_bucket", &self.kubeconfig_bucket_name());
        context.insert("object_storage_logs_bucket", &self.logs_bucket_name());
        // Qovery features
        context.insert("log_history_enabled", &self.context.is_feature_enabled(&Features::LogsHistory));
        context.insert(
            "metrics_history_enabled",
            &self.context.is_feature_enabled(&Features::MetricsHistory),
        );

        // Advanced settings
        context.insert("resource_expiration_in_seconds", &self.advanced_settings().pleco_resources_ttl);

        // Kubernetes
        context.insert("test_cluster", &self.context.is_test_cluster());
        context.insert("kubernetes_cluster_long_id", &self.long_id);
        context.insert("kubernetes_cluster_id", self.id());
        context.insert("kubernetes_cluster_name", self.cluster_name().as_str());
        context.insert("kubernetes_cluster_version", &self.version.to_string());
        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());

        // GCP
        // credentials
        context.insert(
            "gcp_json_credentials_type",
            &self.options.gcp_json_credentials.r#type.to_string(),
        );
        context.insert(
            "gcp_json_credentials_private_key_id",
            &self.options.gcp_json_credentials.private_key_id.to_string(),
        );
        context.insert(
            "gcp_json_credentials_private_key",
            &self
                .options
                .gcp_json_credentials
                .private_key
                .as_str()
                .escape_default() // escape new lines to have \n instead
                .to_string(),
        );
        context.insert(
            "gcp_json_credentials_client_email",
            &self.options.gcp_json_credentials.client_email.to_string(),
        );
        context.insert(
            "gcp_json_credentials_client_id",
            &self.options.gcp_json_credentials.client_id.to_string(),
        );
        context.insert(
            "gcp_json_credentials_auth_uri",
            self.options.gcp_json_credentials.auth_uri.as_str(),
        );
        context.insert(
            "gcp_json_credentials_token_uri",
            self.options.gcp_json_credentials.token_uri.as_str(),
        );
        context.insert(
            "gcp_json_credentials_auth_provider_x509_cert_url",
            self.options.gcp_json_credentials.auth_provider_x509_cert_url.as_str(),
        );
        context.insert(
            "gcp_json_credentials_client_x509_cert_url",
            self.options.gcp_json_credentials.client_x509_cert_url.as_str(),
        );
        context.insert(
            "gcp_json_credentials_universe_domain",
            &self.options.gcp_json_credentials.universe_domain.to_string(),
        );
        context.insert("gcp_project_id", self.options.gcp_json_credentials.project_id.as_str());
        context.insert("gcp_region", &self.region.to_cloud_provider_format());
        context.insert(
            "gcp_zones",
            &self
                .region
                .zones()
                .iter()
                .map(|z| z.to_cloud_provider_format())
                .collect::<Vec<&str>>(),
        );
        let rfc3339_format = format_description::parse("[hour]:[minute]").unwrap_or_default();
        context.insert(
            "cluster_maintenance_start_time",
            &self
                .options
                .cluster_maintenance_start_time
                .format(&rfc3339_format)
                .unwrap_or_default(),
        ); // RFC3339 https://www.ietf.org/rfc/rfc3339.txt
        let cluster_maintenance_end_time = match &self.options.cluster_maintenance_end_time {
            Some(t) => t.format(&rfc3339_format).unwrap_or_default(),
            None => "".to_string(),
        };
        context.insert("cluster_maintenance_end_time", cluster_maintenance_end_time.as_str()); // RFC3339 https://www.ietf.org/rfc/rfc3339.txt

        // VPC
        match &self.options.vpc_mode {
            VpcMode::Automatic => {
                // if automatic, Qovery to create a new VPC for the cluster
                context.insert("vpc_name", self.cluster_name().as_str());
                context.insert("vpc_use_existing", &false);
            }
            VpcMode::ExistingVpc { vpc_name } => {
                // If VPC is provided by client, then reuse it without creating a new VPC for the cluster
                context.insert("vpc_name", vpc_name);
                context.insert("vpc_use_existing", &true);
            }
        }

        // AWS S3 tfstates storage
        context.insert(
            "aws_access_key_tfstates_account",
            match self.cloud_provider().terraform_state_credentials() {
                Some(x) => x.access_key_id.as_str(),
                None => "",
            },
        );
        context.insert(
            "aws_secret_key_tfstates_account",
            match self.cloud_provider().terraform_state_credentials() {
                Some(x) => x.secret_access_key.as_str(),
                None => "",
            },
        );
        context.insert(
            "aws_region_tfstates_account",
            match self.cloud_provider().terraform_state_credentials() {
                Some(x) => x.region.as_str(),
                None => "",
            },
        );
        context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");
        context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");

        // DNS
        let managed_dns_list = vec![self.dns_provider.name()];
        let managed_dns_domains_helm_format = vec![self.dns_provider.domain().to_string()];
        let managed_dns_domains_root_helm_format = vec![self.dns_provider.domain().root_domain().to_string()];
        let managed_dns_domains_terraform_format = terraform_list_format(vec![self.dns_provider.domain().to_string()]);
        let managed_dns_domains_root_terraform_format =
            terraform_list_format(vec![self.dns_provider.domain().root_domain().to_string()]);
        let managed_dns_resolvers_terraform_format = terraform_list_format(
            self.dns_provider()
                .resolvers()
                .iter()
                .map(|x| x.clone().to_string())
                .collect(),
        );

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

        // TLS
        context.insert(
            "acme_server_url",
            LetsEncryptConfig::new(self.options.tls_email_report.to_string(), self.context.is_test_cluster())
                .acme_url()
                .as_str(),
        );

        // Vault
        context.insert("vault_auth_method", "none");

        // TODO(benjaminch): to be removed, we are not supposed to get env from here!!
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
                                event_details.clone(),
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

        Ok(context)
    }

    fn create(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));

        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing GKE cluster deployment.".to_string()),
        ));

        if !self.context().is_first_cluster_deployment() {
            // upgrade cluster instead if required
            match self.get_kubeconfig_file() {
                Ok(path) => match is_kubernetes_upgrade_required(
                    path,
                    self.version.clone(),
                    self.cloud_provider.credentials_environment_variables(),
                    event_details.clone(),
                    self.logger(),
                ) {
                    Ok(kubernetes_upgrade_status) => {
                        if kubernetes_upgrade_status.required_upgrade_on.is_some() {
                            return self.upgrade_with_status(kubernetes_upgrade_status);
                        }

                        self.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                        ))
                    }
                    Err(e) => {
                        // Log a warning, this error is not blocking
                        self.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new("Error detected, upgrade won't occurs, but standard deployment.".to_string(), Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                            )),
                        );
                    }
                },
                Err(_) => self.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Kubernetes cluster upgrade not required, config file is not found and cluster have certainly never been deployed before".to_string())))
            };
        }

        let temp_dir = self.get_temp_dir(event_details.clone())?;
        let qovery_terraform_config_file = format!("{}/qovery-tf-config.json", &temp_dir);

        // generate terraform files and copy them into temp dir
        let context = self.tera_context()?;

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            context,
        ) {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir,
                e,
            )));
        }

        let dirs_to_be_copied_to = vec![
            // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/gcp/bootstrap/common/charts directory.
            // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
            (
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                format!("{}/common/charts", temp_dir.as_str()),
            ),
            // copy lib/common/bootstrap/chart_values directory (and sub directory) into the lib/gcp/bootstrap/common/chart_values directory.
            (
                format!("{}/common/bootstrap/chart_values", self.context.lib_root_dir()),
                format!("{}/common/chart_values", temp_dir.as_str()),
            ),
        ];
        for (source_dir, target_dir) in dirs_to_be_copied_to {
            if let Err(e) = crate::template::copy_non_template_files(&source_dir, target_dir.as_str()) {
                return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    event_details,
                    source_dir,
                    target_dir,
                    e,
                )));
            }
        }

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deploying GKE cluster.".to_string()),
        ));

        // TODO(benjaminch): move this elsewhere
        // Create object-storage buckets
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Create Qovery managed object storage buckets".to_string()),
        ));
        if let Err(e) = self.object_storage.create_bucket(
            self.kubeconfig_bucket_name().as_str(),
            self.advanced_settings.resource_ttl(),
            false,
        ) {
            let error = EngineError::new_object_storage_error(event_details, e);
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(Box::new(error));
        }

        // Logs bucket
        if let Err(e) = self.object_storage.create_bucket(
            self.logs_bucket_name().as_str(),
            self.advanced_settings.resource_ttl(),
            false,
        ) {
            let error = EngineError::new_object_storage_error(event_details, e);
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(Box::new(error));
        }

        // Terraform deployment dedicated to cloud resources
        if let Err(e) = terraform_init_validate_plan_apply(
            temp_dir.as_str(),
            self.context.is_dry_run_deploy(),
            self.cloud_provider().credentials_environment_variables().as_slice(),
        ) {
            return Err(Box::new(EngineError::new_terraform_error(event_details, e)));
        }

        // Retrieve config generated via Terraform
        let qovery_terraform_config: GkeQoveryTerraformConfig = self
            .get_gke_qovery_terraform_config(qovery_terraform_config_file.as_str())
            .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

        // Push config file to object storage
        let kubeconfig_path = &self.get_kubeconfig_file_path()?;
        let kubeconfig_name = self.get_kubeconfig_filename();
        if let Err(e) = self.object_storage.put_object(
            self.kubeconfig_bucket_name().as_str(),
            kubeconfig_name.as_str(),
            kubeconfig_path,
        ) {
            let error = EngineError::new_object_storage_error(event_details, e);
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(Box::new(error));
        }

        // Configure kubectl to be able to connect to cluster
        let _ = self.configure_gcloud_for_cluster(event_details.clone()); // TODO(benjaminch): properly handle this error

        // Ensure all nodes are ready on Kubernetes
        match self.check_workers_on_create() {
            Ok(_) => self.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe("Kubernetes nodes have been successfully created".to_string()),
            )),
            Err(e) => {
                return Err(Box::new(EngineError::new_k8s_node_not_ready(event_details, e)));
            }
        };

        // Update cluster config to vault
        let kubeconfig = fs::read_to_string(kubeconfig_path).map_err(|e| {
            Box::new(EngineError::new_cannot_retrieve_cluster_config_file(
                event_details.clone(),
                CommandError::new_from_safe_message(format!(
                    "Cannot read kubeconfig file {}: {e}",
                    kubeconfig_path.to_str().unwrap_or_default()
                )),
            ))
        })?;
        let kubeconfig_b64 = general_purpose::STANDARD.encode(kubeconfig);
        let cluster_secrets = ClusterSecrets::new_google_gke(ClusterSecretsGcp::new(
            self.options.gcp_json_credentials.clone().into(),
            self.options.gcp_json_credentials.project_id.to_string(),
            self.region.clone(),
            Some(kubeconfig_b64),
            Some(qovery_terraform_config.gke_cluster_public_hostname),
            self.kind(),
            self.cloud_provider().name().to_string(),
            self.long_id().to_string(),
            self.options.grafana_admin_user.clone(),
            self.options.grafana_admin_password.clone(),
            self.cloud_provider.organization_long_id().to_string(),
            self.context().is_test_cluster(),
        ));
        // vault config is not blocking
        if let Err(e) = self.update_gke_vault_config(event_details.clone(), cluster_secrets) {
            self.logger.log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new("Cannot push cluster config to Vault".to_string(), Some(e.to_string())),
            ))
        }

        // kubernetes helm deployments on the cluster
        let credentials_environment_variables: Vec<(String, String)> = self
            .cloud_provider
            .credentials_environment_variables()
            .into_iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect();

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
        ));

        let charts_prerequisites = helm_charts::ChartsConfigPrerequisites::new(
            self.cloud_provider.organization_id().to_string(),
            self.cloud_provider.organization_long_id(),
            self.id().to_string(),
            self.long_id,
            self.region.clone(),
            self.cluster_name(),
            vec![CpuArchitecture::AMD64], // TODO(ENG-1643): GKE integration, introduce ARM
            "gcp".to_string(),
            self.context.is_test_cluster(),
            self.options.gcp_json_credentials.clone(),
            self.options.qovery_engine_location.clone(),
            self.context.is_feature_enabled(&Features::LogsHistory),
            self.context.is_feature_enabled(&Features::MetricsHistory),
            self.context.is_feature_enabled(&Features::Grafana),
            self.dns_provider.domain().root_domain().to_string(),
            self.dns_provider.domain().to_helm_format_string(),
            terraform_list_format(
                self.dns_provider()
                    .resolvers()
                    .iter()
                    .map(|x| x.clone().to_string())
                    .collect(),
            ),
            self.dns_provider.domain().root_domain().to_helm_format_string(),
            self.dns_provider.provider_name().to_string(),
            LetsEncryptConfig::new(self.options.tls_email_report.to_string(), self.context.is_test_cluster()),
            self.dns_provider().provider_configuration(),
            self.context.disable_pleco(),
            self.options.clone(),
            self.advanced_settings().clone(),
        );

        let helm_charts_to_deploy = helm_charts::gcp_helm_charts(
            format!("{}/qovery-tf-config.json", &temp_dir).as_str(),
            &charts_prerequisites,
            Some(&temp_dir),
            kubeconfig_path,
            &credentials_environment_variables,
            &*self.context.qovery_api,
            self.customer_helm_charts_override(),
        )
        .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?;

        deploy_charts_levels(
            &self.kube_client()?,
            kubeconfig_path,
            credentials_environment_variables
                .iter()
                .map(|(l, r)| (l.as_str(), r.as_str()))
                .collect_vec()
                .as_slice(),
            helm_charts_to_deploy,
            self.context.is_dry_run_deploy(),
        )
        .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))
    }

    fn configure_gcloud_for_cluster(&self, event_details: EventDetails) -> Result<(), Box<EngineError>> {
        // Configure kubectl to be able to connect to cluster
        // https://cloud.google.com/kubernetes-engine/docs/how-to/cluster-access-for-kubectl#gcloud_1

        // Get credentials file path
        let temp_dir = self.get_temp_dir(event_details)?;
        let gcp_credentials_file_path = format!("{}/gcp-credentials.json", &temp_dir);

        let _ = QoveryCommand::new(
            "gcloud",
            &[
                "auth",
                "activate-service-account",
                &self.options.gcp_json_credentials.client_email,
                format!("--key-file={}", gcp_credentials_file_path).as_str(),
            ],
            self.cloud_provider.credentials_environment_variables().as_slice(),
        )
        .exec(); // TODO(benjaminch): introduce an EngineError for it and handle it properly

        let _ = QoveryCommand::new(
            "gcloud",
            &[
                "container",
                "clusters",
                "get-credentials",
                &self.name,
                format!("--region={}", self.region.to_cloud_provider_format()).as_str(),
                format!("--project={}", self.options.gcp_json_credentials.project_id).as_str(),
            ],
            self.cloud_provider.credentials_environment_variables().as_slice(),
        )
        .exec(); // TODO(benjaminch): introduce an EngineError for it and handle it properly

        Ok(())
    }

    fn create_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        let kubeconfig_path = self.get_kubeconfig_file()?;
        let environment_variables: Vec<(&str, &str)> = self.cloud_provider.credentials_environment_variables();

        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Infrastructure(InfrastructureStep::Create)),
            EventMessage::new_from_safe("GKE.create_error() called.".to_string()),
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

    fn delete(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        let skip_kubernetes_step = false;

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
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir,
                e,
            )));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/gcp/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/gcp/bootstrap/*.tf files
        let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str())
        {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                bootstrap_charts_dir,
                common_charts_temp_dir,
                e,
            )));
        }

        // should apply before destroy to be sure destroy will compute on all resources
        // don't exit on failure, it can happen if we resume a destroy process
        let message = format!(
            "Ensuring everything is up to date before deleting cluster {}/{}",
            self.name(),
            self.id()
        );
        self.logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Running Terraform apply before running a delete.".to_string()),
        ));

        if let Err(e) = terraform_init_validate_plan_apply(
            temp_dir.as_str(),
            false,
            self.cloud_provider().credentials_environment_variables().as_slice(),
        ) {
            // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
            self.logger().log(EngineEvent::Error(
                EngineError::new_terraform_error(event_details.clone(), e),
                None,
            ));
        };

        let kubeconfig_path = &self.get_kubeconfig_file_path()?;
        let kubeconfig_path = Path::new(kubeconfig_path);

        if !skip_kubernetes_step {
            // Configure kubectl to be able to connect to cluster
            let _ = self.configure_gcloud_for_cluster(event_details.clone()); // TODO(benjaminch): properly handle this error

            // should make the diff between all namespaces and qovery managed namespaces
            let message = format!(
                "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
                self.name(),
                self.id()
            );
            self.logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

            let all_namespaces = kubectl_exec_get_all_namespaces(
                kubeconfig_path,
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

                    // Namespaces which are not deleteable because managed by GKE
                    // Example error: GKE Warden authz [denied by managed-namespaces-limitation]: the namespace "gke-gmp-system" is managed and the request's verb "delete" is denied
                    let undeletable_namespaces = [
                        "gke-gmp-system",
                        "gke-managed-filestorecsi",
                        "gmp-public",
                        "gke-managed-cim",
                    ];
                    for namespace_to_delete in namespaces_to_delete
                        .into_iter()
                        .filter(|ns| !undeletable_namespaces.contains(ns))
                    {
                        match kubectl_exec_delete_namespace(
                            kubeconfig_path,
                            namespace_to_delete,
                            self.cloud_provider().credentials_environment_variables(),
                        ) {
                            Ok(_) => self.logger().log(EngineEvent::Info(
                                event_details.clone(),
                                EventMessage::new_from_safe(format!(
                                    "Namespace `{namespace_to_delete}` deleted successfully."
                                )),
                            )),
                            Err(e) => {
                                if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                                    self.logger().log(EngineEvent::Warning(
                                        event_details.clone(),
                                        EventMessage::new_from_safe(format!(
                                            "Can't delete the namespace `{namespace_to_delete}`"
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
            self.logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

            // delete custom metrics api to avoid stale namespaces on deletion
            let helm = Helm::new(kubeconfig_path, &self.cloud_provider.credentials_environment_variables())
                .map_err(|e| EngineError::new_helm_error(event_details.clone(), e))?;
            let chart = ChartInfo::new_from_release_name("metrics-server", "kube-system");

            if let Err(e) = helm.uninstall(&chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                // this error is not blocking
                self.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new_from_engine_error(EngineError::new_helm_error(event_details.clone(), e)),
                ));
            }

            // required to avoid namespace stuck on deletion
            if let Err(e) = uninstall_cert_manager(
                kubeconfig_path,
                self.cloud_provider().credentials_environment_variables(),
                event_details.clone(),
                self.logger(),
            ) {
                // this error is not blocking, logging a warning and move on
                self.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(
                        "An error occurred while trying to uninstall cert-manager. This is not blocking.".to_string(),
                        Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }

            self.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe("Deleting Qovery managed helm charts".to_string()),
            ));

            let qovery_namespaces = get_qovery_managed_namespaces();
            for qovery_namespace in qovery_namespaces.iter() {
                let charts_to_delete = helm
                    .list_release(Some(qovery_namespace), &[])
                    .map_err(|e| EngineError::new_helm_error(event_details.clone(), e.clone()))?;

                for chart in charts_to_delete {
                    let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                    match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
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
                let deletion = kubectl_exec_delete_namespace(
                    kubeconfig_path,
                    qovery_namespace,
                    self.cloud_provider().credentials_environment_variables(),
                );
                match deletion {
                    Ok(_) => self.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!("Namespace {qovery_namespace} is fully deleted")),
                    )),
                    Err(e) => {
                        if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                            self.logger().log(EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new_from_safe(format!("Can't delete namespace {qovery_namespace}.")),
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
                        match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
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
        self.logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Running Terraform destroy".to_string()),
        ));

        if let Err(err) = terraform_init_validate_destroy(
            temp_dir.as_str(),
            false,
            self.cloud_provider().credentials_environment_variables().as_slice(),
        ) {
            return Err(Box::new(EngineError::new_terraform_error(event_details, err)));
        }

        // delete info on vault
        let vault_conn = QVaultClient::new(event_details.clone());
        if let Ok(vault_conn) = vault_conn {
            let mount = secret_manager::vault::get_vault_mount_name(self.context().is_test_cluster());

            // ignore on failure
            if let Err(e) = vault_conn.delete_secret(mount.as_str(), self.long_id().to_string().as_str()) {
                self.logger.log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new("Cannot delete cluster config from Vault".to_string(), Some(e.to_string())),
                ))
            }
        }

        // delete object storages
        if let Err(e) = self
            .object_storage
            .delete_bucket(&self.kubeconfig_bucket_name(), BucketDeleteStrategy::HardDelete)
        {
            self.logger.log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(
                    format!(
                        "Cannot delete cluster kubeconfig object storage `{}`",
                        &self.kubeconfig_bucket_name()
                    ),
                    Some(e.to_string()),
                ),
            ))
        }
        if let Err(e) = self
            .object_storage
            .delete_bucket(&self.logs_bucket_name(), BucketDeleteStrategy::HardDelete)
        {
            self.logger.log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(
                    format!("Cannot delete cluster logs object storage `{}`", &self.logs_bucket_name()),
                    Some(e.to_string()),
                ),
            ))
        }

        self.logger().log(EngineEvent::Info(
            event_details,
            EventMessage::new_from_safe("Kubernetes cluster successfully deleted".to_string()),
        ));

        Ok(())
    }

    fn pause(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));

        // Configure kubectl to be able to connect to cluster
        let _ = self.configure_gcloud_for_cluster(event_details.clone()); // TODO(benjaminch): properly handle this error

        // avoid clippy yelling about `get_engine_location` not used
        let _ = self.get_engine_location();

        Ok(())
    }

    fn pause_error(&self) -> Result<(), Box<EngineError>> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Infrastructure(InfrastructureStep::Pause)),
            EventMessage::new_from_safe("GKE.pause_error() called.".to_string()),
        ));

        Ok(())
    }

    fn get_gke_qovery_terraform_config(
        &self,
        qovery_terraform_config_file: &str,
    ) -> Result<GkeQoveryTerraformConfig, TerraformError> {
        let content_file = match File::open(qovery_terraform_config_file) {
            Ok(x) => x,
            Err(e) => {
                return Err(TerraformError::ConfigFileNotFound {
                    path: qovery_terraform_config_file.to_string(),
                    raw_message: e.to_string(),
                });
            }
        };

        let reader = BufReader::new(content_file);
        match serde_json::from_reader(reader) {
            Ok(config) => Ok(config),
            Err(e) => Err(TerraformError::ConfigFileInvalidContent {
                path: qovery_terraform_config_file.to_string(),
                raw_message: e.to_string(),
            }),
        }
    }

    fn update_gke_vault_config(
        &self,
        event_details: EventDetails,
        cluster_secrets: ClusterSecrets,
    ) -> Result<(), Box<EngineError>> {
        let vault_conn = match QVaultClient::new(event_details.clone()) {
            Ok(x) => Some(x),
            Err(_) => None,
        };
        if let Some(vault) = vault_conn {
            let _ = cluster_secrets.create_or_update_secret(&vault, false, event_details.clone());
        };

        Ok(())
    }
}

impl Kubernetes for Gke {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Gke
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
        let zones = self.region.zones();
        if !zones.is_empty() {
            return Some(zones.iter().map(|z| z.to_cloud_provider_format()).collect());
        }
        None
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider.as_ref()
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider.as_ref()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.as_ref()
    }

    fn metrics_registry(&self) -> &dyn MetricsRegistry {
        self.metrics_registry.as_ref()
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.object_storage
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        Ok(()) // TODO(benjaminch): add some checks eventually
    }

    fn is_network_managed_by_user(&self) -> bool {
        false // TODO(benjaminch): GKE integration, to be checked
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        // TODO(ENG-1643): GKE integration, add ARM support
        vec![CpuArchitecture::AMD64]
    }

    #[named]
    fn on_create(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider.kind().to_string().to_lowercase().as_str(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || self.create())
    }

    #[named]
    fn on_create_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider.kind().to_string().to_lowercase().as_str(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || self.create_error())
    }

    fn upgrade_with_status(&self, _kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), Box<EngineError>> {
        Ok(()) // TODO(benjaminch): GKE integration
    }

    fn on_upgrade(&self) -> Result<(), Box<EngineError>> {
        Ok(()) // TODO(benjaminch): GKE integration
    }

    fn on_upgrade_error(&self) -> Result<(), Box<EngineError>> {
        Ok(()) // TODO(benjaminch): GKE integration
    }

    #[named]
    fn on_pause(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            self.cloud_provider.kind().to_string().to_lowercase().as_str(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || self.pause())
    }

    #[named]
    fn on_pause_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            self.cloud_provider.kind().to_string().to_lowercase().as_str(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || self.pause_error())
    }

    #[named]
    fn on_delete(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            self.cloud_provider.kind().to_string().to_lowercase().as_str(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || self.delete())
    }

    fn on_delete_error(&self) -> Result<(), Box<EngineError>> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete)),
            EventMessage::new_from_safe(format!(
                "{}.delete_error() called.",
                self.cloud_provider.kind().to_string().to_lowercase().as_str(),
            )),
        ));

        Ok(())
    }

    fn update_vault_config(
        &self,
        event_details: EventDetails,
        _qovery_terraform_config_file: String,
        cluster_secrets: ClusterSecrets,
        _kubeconfig_file_path: Option<&Path>,
    ) -> Result<(), Box<EngineError>> {
        self.update_gke_vault_config(event_details, cluster_secrets)
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn customer_helm_charts_override(&self) -> Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>> {
        self.customer_helm_charts_override.clone()
    }

    fn get_kubernetes_connection(&self) -> Option<String> {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GkeQoveryTerraformConfig {
    pub gke_cluster_public_hostname: String,
}
