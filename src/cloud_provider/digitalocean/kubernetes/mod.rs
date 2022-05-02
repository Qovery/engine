use std::borrow::Borrow;
use std::env;
use std::fs::File;

use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::aws::regions::AwsZones;
use crate::cloud_provider::digitalocean::do_api_common::{do_get_from_api, DoApiType};
use crate::cloud_provider::digitalocean::kubernetes::doks_api::{
    get_do_kubeconfig_by_cluster_name, get_do_latest_doks_slug_from_api, get_doks_info_from_name,
};
use crate::cloud_provider::digitalocean::kubernetes::helm_charts::{do_helm_charts, ChartsConfigPrerequisites};
use crate::cloud_provider::digitalocean::kubernetes::node::DoInstancesType;
use crate::cloud_provider::digitalocean::models::doks::KubernetesCluster;
use crate::cloud_provider::digitalocean::network::load_balancer::do_get_load_balancer_ip;
use crate::cloud_provider::digitalocean::network::vpc::{
    get_do_random_available_subnet_from_api, get_do_vpc_name_available_from_api, VpcInitKind,
};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::helm::{deploy_charts_levels, ChartInfo, ChartSetValue, HelmChartNamespaces};
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, send_progress_on_long_task, uninstall_cert_manager, Kind, Kubernetes,
    KubernetesUpgradeStatus, ProviderOptions,
};
use crate::cloud_provider::models::NodeGroups;
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd::helm::{to_engine_error, Helm};
use crate::cmd::kubectl::{
    do_kubectl_exec_get_loadbalancer_id, kubectl_exec_get_all_namespaces, kubectl_exec_get_events,
};
use crate::cmd::terraform::{terraform_exec, terraform_init_validate_plan_apply, terraform_init_validate_state_list};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{
    EngineEvent, EnvironmentStep, EventDetails, EventMessage, GeneralStep, InfrastructureStep, Stage, Transmitter,
};
use crate::io_models::{
    Action, Context, Features, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel,
    ProgressScope, QoveryIdentifier, StringPath, ToHelmString,
};
use crate::logger::Logger;
use crate::models::digital_ocean::DoRegion;
use crate::models::types::VersionsNumber;
use crate::object_storage::spaces::{BucketDeleteStrategy, Spaces};
use crate::object_storage::ObjectStorage;
use crate::runtime::block_on;
use crate::string::terraform_list_format;
use crate::{cmd, dns_provider};
use ::function_name::named;
use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::OperationResult;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub mod cidr;
pub mod doks_api;
pub mod helm_charts;
pub mod node;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DoksOptions {
    // Digital Ocean
    pub vpc_cidr_block: String,
    pub vpc_name: String,
    pub vpc_cidr_set: VpcInitKind,
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    pub qovery_cluster_secret_token: String,
    pub qovery_engine_location: EngineLocation,
    pub engine_version_controller_token: String,
    pub agent_version_controller_token: String,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub discord_api_key: String,
    pub qovery_nats_url: String,
    pub qovery_nats_user: String,
    pub qovery_nats_password: String,
    pub qovery_ssh_key: String,
    // Others
    pub tls_email_report: String,
}

impl ProviderOptions for DoksOptions {}

pub struct DOKS {
    context: Context,
    id: String,
    long_id: uuid::Uuid,
    name: String,
    version: String,
    region: DoRegion,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    nodes_groups: Vec<NodeGroups>,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    spaces: Spaces,
    template_directory: String,
    options: DoksOptions,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl DOKS {
    pub fn new(
        context: Context,
        id: String,
        long_id: uuid::Uuid,
        name: String,
        version: String,
        region: DoRegion,
        cloud_provider: Arc<Box<dyn CloudProvider>>,
        dns_provider: Arc<Box<dyn DnsProvider>>,
        nodes_groups: Vec<NodeGroups>,
        options: DoksOptions,
        logger: Box<dyn Logger>,
    ) -> Result<Self, EngineError> {
        let template_directory = format!("{}/digitalocean/bootstrap", context.lib_root_dir());

        for node_group in &nodes_groups {
            if let Err(e) = DoInstancesType::from_str(node_group.instance_type.as_str()) {
                let err = EngineError::new_unsupported_instance_type(
                    EventDetails::new(
                        Some(cloud_provider.kind()),
                        QoveryIdentifier::new_from_long_id(context.organization_id().to_string()),
                        QoveryIdentifier::new_from_long_id(context.cluster_id().to_string()),
                        QoveryIdentifier::new_from_long_id(context.execution_id().to_string()),
                        Some(region.to_string()),
                        Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
                        Transmitter::Kubernetes(id, name),
                    ),
                    node_group.instance_type.as_str(),
                    e,
                );

                logger.log(EngineEvent::Error(err.clone(), None));

                return Err(err);
            }
        }

        let spaces = Spaces::new(
            context.clone(),
            "spaces-temp-id".to_string(),
            "my-spaces-object-storage".to_string(),
            cloud_provider.access_key_id(),
            cloud_provider.secret_access_key(),
            region,
            BucketDeleteStrategy::HardDelete,
        );

        let listeners = cloud_provider.listeners().clone();
        Ok(DOKS {
            context,
            id,
            long_id,
            name,
            version,
            region,
            cloud_provider,
            dns_provider,
            spaces,
            options,
            nodes_groups,
            template_directory,
            logger,
            listeners,
        })
    }

    fn kubeconfig_bucket_name(&self) -> String {
        format!("qovery-kubeconfigs-{}", self.id())
    }

    fn logs_bucket_name(&self) -> String {
        format!("qovery-logs-{}", self.id)
    }

    fn kubeconfig_file_name(&self) -> String {
        format!("{}.yaml", self.id)
    }

    // TODO(benjaminch): Very dirty quickfix, should be removed and cluster id / name should be handled globally
    fn doks_cluster_name(&self) -> String {
        format!("qovery-{}", self.id)
    }

    // create a context to render tf files (terraform) contained in lib/digitalocean/
    fn tera_context(&self) -> Result<TeraContext, EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::LoadConfiguration));
        let mut context = TeraContext::new();

        // Digital Ocean
        context.insert("digitalocean_token", &self.cloud_provider.token());
        context.insert("do_region", &self.region.to_string());

        // Digital Ocean: Spaces Credentials
        context.insert("spaces_access_id", &self.cloud_provider.access_key_id());
        context.insert("spaces_secret_key", &self.cloud_provider.secret_access_key());

        let space_kubeconfig_bucket = format!("qovery-kubeconfigs-{}", self.id.as_str());
        context.insert("space_bucket_kubeconfig", &space_kubeconfig_bucket);

        // Digital Ocean: Network
        context.insert("do_vpc_name", self.options.vpc_name.as_str());
        let vpc_cidr_block = match self.options.vpc_cidr_set {
            // VPC subnet is not set, getting a non used subnet
            VpcInitKind::Autodetect => {
                match get_do_vpc_name_available_from_api(self.cloud_provider.token(), self.options.vpc_name.clone()) {
                    Ok(vpcs) => match vpcs {
                        // new vpc: select a random non used subnet
                        None => {
                            match get_do_random_available_subnet_from_api(self.cloud_provider.token(), self.region) {
                                Ok(x) => x,
                                Err(e) => return Err(EngineError::new_cannot_get_any_available_vpc(event_details, e)),
                            }
                        }
                        // existing vpc: assign current subnet in this case
                        Some(vpc) => vpc.ip_range,
                    },
                    Err(e) => return Err(EngineError::new_cannot_get_any_available_vpc(event_details, e)),
                }
            }
            VpcInitKind::Manual => self.options.vpc_cidr_block.clone(),
        };
        context.insert("do_vpc_cidr_block", vpc_cidr_block.as_str());
        context.insert("do_vpc_cidr_set", self.options.vpc_cidr_set.to_string().as_str());

        // DNS
        let managed_dns_list = vec![self.dns_provider.name()];
        let managed_dns_domains_helm_format = vec![self.dns_provider.domain().to_string()];
        let managed_dns_domains_root_helm_format = vec![self.dns_provider.domain().root_domain().to_string()];
        let managed_dns_domains_terraform_format = terraform_list_format(vec![self.dns_provider.domain().to_string()]);
        let managed_dns_domains_root_terraform_format =
            terraform_list_format(vec![self.dns_provider.domain().root_domain().to_string()]);
        let managed_dns_resolvers_terraform_format = self.managed_dns_resolvers_terraform_format();

        context.insert("managed_dns", &managed_dns_list);
        context.insert("do_loadbalancer_hostname", &self.do_loadbalancer_hostname());
        context.insert("managed_dns_domain", self.dns_provider.domain().to_string().as_str());
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
        match self.dns_provider.kind() {
            dns_provider::Kind::Cloudflare => {
                context.insert("external_dns_provider", self.dns_provider.provider_name());
                context.insert("cloudflare_api_token", self.dns_provider.token());
                context.insert("cloudflare_email", self.dns_provider.account());
            }
        };

        context.insert("dns_email_report", &self.options.tls_email_report);

        // DOKS
        context.insert("test_cluster", &self.context.is_test_cluster());
        context.insert("doks_cluster_id", &self.id());
        context.insert("doks_master_name", &self.name());
        context.insert("doks_version", self.get_supported_doks_version(event_details.clone())?.as_str());
        context.insert("do_space_kubeconfig_filename", &self.kubeconfig_file_name());

        // Network
        context.insert("vpc_name", self.options.vpc_name.as_str());
        context.insert("vpc_cidr_block", self.options.vpc_cidr_block.as_str());

        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("object_storage_kubeconfig_bucket", &self.kubeconfig_bucket_name());
        context.insert("object_storage_logs_bucket", &self.logs_bucket_name());

        context.insert("engine_version_controller_token", &self.options.engine_version_controller_token);

        context.insert("agent_version_controller_token", &self.options.agent_version_controller_token);

        context.insert("test_cluster", &self.context.is_test_cluster());
        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());
        context.insert("qovery_nats_url", self.options.qovery_nats_url.as_str());
        context.insert("qovery_nats_user", self.options.qovery_nats_user.as_str());
        context.insert("qovery_nats_password", self.options.qovery_nats_password.as_str());
        context.insert("qovery_ssh_key", self.options.qovery_ssh_key.as_str());
        context.insert("discord_api_key", self.options.discord_api_key.as_str());

        // Qovery features
        context.insert("log_history_enabled", &self.context.is_feature_enabled(&Features::LogsHistory));
        context.insert(
            "metrics_history_enabled",
            &self.context.is_feature_enabled(&Features::MetricsHistory),
        );
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert("resource_expiration_in_seconds", &self.context.resource_expiration_in_seconds())
        }

        // grafana credentials
        context.insert("grafana_admin_user", self.options.grafana_admin_user.as_str());

        context.insert("grafana_admin_password", self.options.grafana_admin_password.as_str());

        // TLS
        context.insert("acme_server_url", &self.lets_encrypt_url());

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

        context.insert("nginx_enable_horizontal_autoscaler", "true");
        context.insert("nginx_minimum_replicas", "2");
        context.insert("nginx_maximum_replicas", "2");
        context.insert("nginx_requests_cpu", "250m");
        context.insert("nginx_requests_memory", "384Mi");
        context.insert("nginx_limit_cpu", "1");
        context.insert("nginx_limit_memory", "384Mi");
        context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");
        context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");

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

        // kubernetes workers
        context.insert("doks_worker_nodes", &self.nodes_groups);

        Ok(context)
    }

    fn get_supported_doks_version(&self, event_details: EventDetails) -> Result<String, EngineError> {
        match self.get_doks_info_from_name_api() {
            Ok(x) => Ok(x.version),
            Err(_) => {
                // Might be a new cluster, we check the wished version is supported by DO
                match get_do_latest_doks_slug_from_api(self.cloud_provider.token(), self.version()) {
                    Ok(version) => match version {
                        None => Err(EngineError::new_unsupported_version_error(
                            event_details,
                            self.kind().to_string(),
                            VersionsNumber::from_str(&self.version)
                                .expect("cannot parse version")
                                .to_string(),
                        )),
                        Some(v) => Ok(v),
                    },
                    Err(e) => Err(EngineError::new_cannot_get_supported_versions_error(
                        event_details,
                        self.kind().to_string(),
                        e,
                    )),
                }
            }
        }
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

    fn do_loadbalancer_hostname(&self) -> String {
        format!("qovery-nginx-{}.{}", self.cloud_provider.id(), self.dns_provider().domain())
    }

    fn lets_encrypt_url(&self) -> String {
        match &self.context.is_test_cluster() {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        }
        .to_string()
    }

    // return cluster info from name if exists
    fn get_doks_info_from_name_api(&self) -> Result<KubernetesCluster, CommandError> {
        let api_url = format!("{}/clusters", DoApiType::Doks.api_url());
        let json_content = do_get_from_api(self.cloud_provider.token(), DoApiType::Doks, api_url)?;
        // TODO(benjaminch): `qovery-` to be added into Rust name directly everywhere
        match get_doks_info_from_name(json_content.as_str(), format!("qovery-{}", self.id())) {
            Ok(cluster_result) => match cluster_result {
                None => Err(CommandError::new_from_safe_message(
                    "Cluster doesn't exist on DO side.".to_string(),
                )),
                Some(cluster) => Ok(cluster),
            },
            Err(e) => Err(e),
        }
    }

    fn create(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "start to create Digital Ocean Kubernetes cluster {} with id {}",
                self.name(),
                self.id()
            )),
            self.context.execution_id(),
        ));
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing cluster deployment.".to_string()),
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
                    self.logger().log(EngineEvent::Error(e, None));
                    self.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(
                            "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                        ),
                    ));
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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/digitalocean/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/digitalocean/bootstrap/*.tf files
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
            EventMessage::new_from_safe("Deploying cluster.".to_string()),
        ));
        self.send_to_customer(
            format!("Deploying DOKS {} cluster deployment with id {}", self.name(), self.id()).as_str(),
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
                                Ok(_) => self.logger().log(EngineEvent::Info(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(format!("Successfully removed {}", &entry)),
                                )),
                                Err(e) => {
                                    return Err(EngineError::new_terraform_cannot_remove_entry_out(
                                        event_details,
                                        entry.to_string(),
                                        e,
                                    ))
                                }
                            }
                        };
                    }
                }
            }
            Err(e) => self.logger().log(EngineEvent::Error(
                EngineError::new_terraform_state_does_not_exist(event_details.clone(), e),
                None,
            )),
        };

        // Logs bucket
        if let Err(e) = self.spaces.create_bucket(self.logs_bucket_name().as_str()) {
            let error =
                EngineError::new_object_storage_cannot_create_bucket_error(event_details, self.logs_bucket_name(), e);
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(error);
        }

        // terraform deployment dedicated to cloud resources
        if let Err(e) = terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            return Err(EngineError::new_terraform_error_while_executing_pipeline(event_details, e));
        }

        let kubeconfig_path = &self.get_kubeconfig_file_path()?;
        let kubeconfig_path = Path::new(kubeconfig_path);

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

        let doks_id = match self.get_doks_info_from_name_api() {
            Ok(cluster) => cluster.id,
            Err(e) => return Err(EngineError::new_cannot_get_cluster_error(event_details, e)),
        };

        let charts_prerequisites = ChartsConfigPrerequisites {
            organization_id: self.cloud_provider.organization_id().to_string(),
            organization_long_id: self.cloud_provider.organization_long_id(),
            infra_options: self.options.clone(),
            cluster_id: self.id.clone(),
            cluster_long_id: self.long_id,
            do_cluster_id: doks_id,
            region: self.region(),
            cluster_name: self.cluster_name(),
            cloud_provider: "digitalocean".to_string(),
            test_cluster: self.context.is_test_cluster(),
            do_token: self.cloud_provider.token().to_string(),
            do_space_access_id: self.cloud_provider.access_key_id(),
            do_space_secret_key: self.cloud_provider.secret_access_key(),
            do_space_bucket_kubeconfig: self.kubeconfig_bucket_name(),
            do_space_kubeconfig_filename: self.kubeconfig_file_name(),
            qovery_engine_location: self.options.qovery_engine_location.clone(),
            ff_log_history_enabled: self.context.is_feature_enabled(&Features::LogsHistory),
            ff_metrics_history_enabled: self.context.is_feature_enabled(&Features::MetricsHistory),
            managed_dns_name: self.dns_provider.domain().to_string(),
            managed_dns_helm_format: self.dns_provider.domain().to_helm_format_string(),
            managed_dns_resolvers_terraform_format: self.managed_dns_resolvers_terraform_format(),
            external_dns_provider: self.dns_provider.provider_name().to_string(),
            dns_email_report: self.options.tls_email_report.clone(),
            acme_url: self.lets_encrypt_url(),
            cloudflare_email: self.dns_provider.account().to_string(),
            cloudflare_api_token: self.dns_provider.token().to_string(),
            disable_pleco: self.context.disable_pleco(),
        };

        let chart_prefix_path = &temp_dir;

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
        ));
        let helm_charts_to_deploy = do_helm_charts(
            format!("{}/qovery-tf-config.json", &temp_dir).as_str(),
            &charts_prerequisites,
            Some(chart_prefix_path),
        )
        .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?;

        deploy_charts_levels(
            kubeconfig_path,
            &credentials_environment_variables,
            helm_charts_to_deploy,
            self.context.is_dry_run_deploy(),
        )
        .map_err(|e| EngineError::new_helm_charts_deploy_error(event_details.clone(), e))?;

        // https://github.com/digitalocean/digitalocean-cloud-controller-manager/blob/master/docs/controllers/services/annotations.md#servicebetakubernetesiodo-loadbalancer-hostname
        // it can't be done earlier as nginx ingress is not yet deployed
        // required as load balancer do not have hostname (only IP) and are blocker to get a TLS certificate
        let nginx_ingress_loadbalancer_id = match do_kubectl_exec_get_loadbalancer_id(
                &kubeconfig_path,
                "nginx-ingress",
                "nginx-ingress-ingress-nginx-controller",
                self.cloud_provider.credentials_environment_variables(),
            ) {
                Ok(x) => match x {
                    None => return Err(EngineError::new_k8s_loadbalancer_configuration_issue(event_details.clone(), CommandError::new_from_safe_message("No associated Load balancer UUID was found on DigitalOcean API and it's required for TLS setup.".to_string()))),
                    Some(uuid) => uuid,
                },
                Err(e) => {
                    return Err(EngineError::new_k8s_loadbalancer_configuration_issue(event_details.clone(), e))
                }
            };

        let nginx_ingress_loadbalancer_ip = match do_get_load_balancer_ip(
            self.cloud_provider.token(),
            nginx_ingress_loadbalancer_id.as_str(),
        ) {
            Ok(x) => x.to_string(),
            Err(e) => {
                return Err(EngineError::new_k8s_loadbalancer_configuration_issue(
                    event_details.clone(),
                    CommandError::new(
                        format!("Load balancer IP wasn't able to be retrieved from UUID on DigitalOcean API and it's required for TLS setup, {}", e.message_safe()),
                    e.message_raw(),
                        e.env_vars(),
                )));
            }
        };

        let chart_path = |x: &str| -> String { format!("{}/{}", &chart_prefix_path, x) };
        let load_balancer_dns_hostname = ChartInfo {
            name: "nginx-ingress-dns".to_string(),
            path: chart_path("common/charts/external-name-svc"),
            namespace: HelmChartNamespaces::NginxIngress,
            values: vec![
                ChartSetValue {
                    key: "serviceName".to_string(),
                    value: "nginx-ingress-dns".to_string(),
                },
                ChartSetValue {
                    key: "source".to_string(),
                    value: self.do_loadbalancer_hostname(),
                },
                ChartSetValue {
                    key: "destination".to_string(),
                    value: nginx_ingress_loadbalancer_ip,
                },
            ],
            ..Default::default()
        };

        let helm = Helm::new(&kubeconfig_path, &self.cloud_provider.credentials_environment_variables())
            .map_err(|e| EngineError::new_helm_error(event_details.clone(), e))?;

        // This will ony print the diff on stdout
        let _ = helm.upgrade_diff(&load_balancer_dns_hostname, &[]);
        helm.upgrade(&load_balancer_dns_hostname, &[])
            .map_err(|e| EngineError::new_helm_error(event_details.clone(), e))
    }

    fn create_error(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
        let (kubeconfig_path, _) = self.get_kubeconfig_file()?;
        let environment_variables: Vec<(&str, &str)> = self.cloud_provider.credentials_environment_variables();

        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create)),
            EventMessage::new_from_safe("DOKS.create_error() called.".to_string()),
        ));

        match kubectl_exec_get_events(kubeconfig_path, None, environment_variables) {
            Ok(ok_line) => self
                .logger()
                .log(EngineEvent::Warning(event_details, EventMessage::new(ok_line, None))),
            Err(err) => self.logger().log(EngineEvent::Warning(
                event_details,
                EventMessage::new(
                    "Error trying to get kubernetes events".to_string(),
                    Some(err.message(ErrorMessageVerbosity::FullDetails)),
                ),
            )),
        };

        Ok(())
    }

    fn upgrade_error(&self) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade)),
            EventMessage::new_from_safe("DOKS.upgrade_error() called.".to_string()),
        ));

        Ok(())
    }

    fn downgrade(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn downgrade_error(&self) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Stage::Infrastructure(InfrastructureStep::Downgrade)),
            EventMessage::new_from_safe("DOKS.downgrade_error() called.".to_string()),
        ));

        Ok(())
    }

    fn pause(&self) -> Result<(), EngineError> {
        todo!()
    }

    fn pause_error(&self) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
            EventMessage::new_from_safe("DOKS.pause_error() called.".to_string()),
        ));

        Ok(())
    }

    fn delete(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let skip_kubernetes_step = false;
        self.send_to_customer(
            format!("Preparing to delete DOKS cluster {} with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing to delete cluster.".to_string()),
        ));

        let temp_dir = match self.get_temp_dir(event_details.clone()) {
            Ok(dir) => dir,
            Err(e) => return Err(e),
        };

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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/digitalocean/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/digital/bootstrap/*.tf files
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
        if let Err(e) = cmd::terraform::terraform_init_validate_plan_apply(temp_dir.as_str(), false) {
            // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
            self.logger().log(EngineEvent::Error(
                EngineError::new_terraform_error_while_executing_pipeline(event_details.clone(), e),
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
                    self.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(
                            "Error while getting all namespaces for Kubernetes cluster".to_string(),
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
                        Err(e) => self.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new(format!("Can't delete chart `{}`", chart.name), Some(e.to_string())),
                        )),
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
                            Err(e) => self.logger().log(EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new(
                                    format!("Error deleting chart `{}`", chart.name),
                                    Some(e.to_string()),
                                ),
                            )),
                        }
                    }
                }
                Err(e) => self.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new("Unable to get helm list".to_string(), Some(e.to_string())),
                )),
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
                self.logger().log(EngineEvent::Info(
                    event_details,
                    EventMessage::new_from_safe("Kubernetes cluster successfully deleted".to_string()),
                ));
                Ok(())
            }
            Err(Operation { error, .. }) => Err(EngineError::new_terraform_error_while_executing_destroy_pipeline(
                event_details,
                error,
            )),
            Err(retry::Error::Internal(msg)) => Err(EngineError::new_terraform_error_while_executing_destroy_pipeline(
                event_details,
                CommandError::new("Error while performing Terraform destroy.".to_string(), Some(msg), None),
            )),
        }
    }

    fn delete_error(&self) -> Result<(), EngineError> {
        self.logger().log(EngineEvent::Warning(
            self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete)),
            EventMessage::new_from_safe("DOKS.delete_error() called.".to_string()),
        ));

        Ok(())
    }

    fn cloud_provider_name(&self) -> &str {
        "digitalocean"
    }

    fn struct_name(&self) -> &str {
        "kubernetes"
    }
}

impl Kubernetes for DOKS {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Doks
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
        self.region.to_string()
    }

    fn zone(&self) -> &str {
        ""
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
        &self.spaces
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn get_kubeconfig_file(&self) -> Result<(String, File), EngineError> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        let bucket_name = format!("qovery-kubeconfigs-{}", self.id());
        let object_key = self.get_kubeconfig_filename();
        let stage = Stage::General(GeneralStep::RetrieveClusterConfig);

        // check if kubeconfig locally exists
        let local_kubeconfig = match self.get_temp_dir(event_details.clone()) {
            Ok(x) => {
                let local_kubeconfig_folder_path = format!("{}/{}", &x, &bucket_name);
                let local_kubeconfig_generated = format!("{}/{}", &local_kubeconfig_folder_path, &object_key);
                if Path::new(&local_kubeconfig_generated).exists() {
                    match File::open(&local_kubeconfig_generated) {
                        Ok(_) => Some(local_kubeconfig_generated),
                        Err(err) => {
                            self.logger().log(EngineEvent::Debug(
                                self.get_event_details(stage),
                                EventMessage::new(
                                    format!("Error, couldn't open {} file", &local_kubeconfig_generated),
                                    Some(err.to_string()),
                                ),
                            ));
                            None
                        }
                    }
                } else {
                    None
                }
            }
            Err(_) => None,
        };

        // otherwise, try to get it from digital ocean api
        let result = match local_kubeconfig {
            Some(local_kubeconfig_generated) => match File::open(&local_kubeconfig_generated) {
                Ok(file) => Ok((StringPath::from(&local_kubeconfig_generated), file)),
                Err(e) => Err(EngineError::new_cannot_retrieve_cluster_config_file(
                    event_details.clone(),
                    CommandError::new(
                        "Error while trying to open Kubeconfig file.".to_string(),
                        Some(e.to_string()),
                        None,
                    ),
                )),
            },
            None => {
                let kubeconfig = match get_do_kubeconfig_by_cluster_name(
                    self.cloud_provider.token(),
                    self.doks_cluster_name().as_str(),
                ) {
                    Ok(kubeconfig) => match kubeconfig {
                        None => {
                            return Err(EngineError::new_cannot_retrieve_cluster_config_file(
                                event_details,
                                CommandError::new_from_safe_message("Kubeconfig is empty".to_string()),
                            ))
                        }
                        Some(content) => content,
                    },
                    Err(e) => return Err(EngineError::new_cannot_retrieve_cluster_config_file(event_details, e)),
                };

                let workspace_directory = crate::fs::workspace_directory(
                    self.context().workspace_root_dir(),
                    self.context().execution_id(),
                    format!("object-storage/spaces/{}", self.name()),
                )
                .map_err(|err| {
                    EngineError::new_cannot_retrieve_cluster_config_file(
                        event_details.clone(),
                        CommandError::new(
                            "Error while trying to create workspace directory.".to_string(),
                            Some(err.to_string()),
                            None,
                        ),
                    )
                })
                .expect("Unable to create directory");

                let file_path = format!("{}/qovery-kubeconfigs-{}/{}.yaml", workspace_directory, self.id(), self.id());
                let path = Path::new(file_path.as_str());
                let parent_dir = path.parent().unwrap();
                let _ = block_on(tokio::fs::create_dir_all(parent_dir));

                match block_on(
                    tokio::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(path),
                ) {
                    Ok(mut created_file) => match block_on(created_file.write_all(kubeconfig.as_bytes())) {
                        Ok(_) => {
                            let file = File::open(path).unwrap();
                            Ok((file_path, file))
                        }
                        Err(e) => Err(EngineError::new_cannot_retrieve_cluster_config_file(
                            event_details.clone(),
                            CommandError::new(
                                "Error while trying to write Kubeconfig file content".to_string(),
                                Some(e.to_string()),
                                None,
                            ),
                        )),
                    },
                    Err(e) => Err(EngineError::new_cannot_create_file(
                        event_details.clone(),
                        CommandError::new(
                            "Error while trying to create Kubeconfig file.".to_string(),
                            Some(e.to_string()),
                            None,
                        ),
                    )),
                }
            }
        };

        match result {
            Err(e) => Err(EngineError::new_cannot_retrieve_cluster_config_file(
                event_details,
                e.underlying_error().unwrap_or_default(),
            )),
            Ok((file_path, file)) => Ok((file_path, file)),
        }
    }

    fn get_kubeconfig_file_path(&self) -> Result<String, EngineError> {
        let (path, _) = self.get_kubeconfig_file()?;
        Ok(path)
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
                "Start preparing DOKS upgrade process {} cluster with id {}",
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

        if let Err(e) = self.delete_crashlooping_pods(
            None,
            None,
            Some(3),
            self.cloud_provider().credentials_environment_variables(),
            event_details.stage().clone(),
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

        let upgrade_doks_version = match get_do_latest_doks_slug_from_api(self.cloud_provider.token(), self.version()) {
            Ok(version) => match version {
                None => {
                    return Err(EngineError::new_unsupported_version_error(
                        event_details,
                        self.kind().to_string(),
                        VersionsNumber::from_str(&self.version)
                            .expect("cannot parse version")
                            .to_string(),
                    ))
                }
                Some(v) => v,
            },
            Err(e) => {
                return Err(EngineError::new_cannot_get_supported_versions_error(
                    event_details,
                    self.kind().to_string(),
                    e,
                ))
            }
        };

        context.insert("doks_version", (&upgrade_doks_version).to_string().as_str());

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

        self.send_to_customer(
            format!("Upgrading Kubernetes {} nodes", self.name()).as_str(),
            &listeners_helper,
        );
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Upgrading Kubernetes nodes.".to_string()),
        ));

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
                return Err(EngineError::new_terraform_error_while_executing_pipeline(event_details, e));
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
        kubernetes::deploy_environment(self, environment, event_details, self.logger())
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
        kubernetes::deploy_environment_error(self, environment, event_details, self.logger())
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
        kubernetes::pause_environment(self, environment, event_details, self.logger())
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
        kubernetes::delete_environment(self, environment, event_details, self.logger())
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

impl Listen for DOKS {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
