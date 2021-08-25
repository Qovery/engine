use std::env;

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::digitalocean::application::Region;
use crate::cloud_provider::digitalocean::do_api_common::{do_get_from_api, DoApiType};
use crate::cloud_provider::digitalocean::kubernetes::doks_api::{
    get_do_latest_doks_slug_from_api, get_doks_info_from_name,
};
use crate::cloud_provider::digitalocean::kubernetes::helm_charts::{do_helm_charts, ChartsConfigPrerequisites};
use crate::cloud_provider::digitalocean::kubernetes::node::Node;
use crate::cloud_provider::digitalocean::models::doks::KubernetesCluster;
use crate::cloud_provider::digitalocean::network::vpc::{
    get_do_random_available_subnet_from_api, get_do_vpc_name_available_from_api, VpcInitKind,
};
use crate::cloud_provider::digitalocean::DO;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::helm::deploy_charts_levels;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesNode};
use crate::cloud_provider::models::WorkerNodeDataTemplate;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd::terraform::{terraform_exec, terraform_init_validate_plan_apply, terraform_init_validate_state_list};
use crate::dns_provider;
use crate::dns_provider::DnsProvider;
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope, SimpleError};
use crate::fs::workspace_directory;
use crate::models::{
    Context, Features, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::object_storage::spaces::Spaces;
use crate::object_storage::ObjectStorage;
use crate::string::terraform_list_format;
use std::path::PathBuf;

pub mod cidr;
pub mod doks_api;
pub mod helm_charts;
pub mod node;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DoksOptions {
    // Digital Ocean
    pub vpc_cidr_block: String,
    pub vpc_name: String,
    pub vpc_cidr_set: VpcInitKind,
    // Qovery
    pub qovery_api_url: String,
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

pub struct DOKS<'a> {
    context: Context,
    id: String,
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a DO,
    nodes: Vec<Node>,
    dns_provider: &'a dyn DnsProvider,
    spaces: Spaces,
    template_directory: String,
    options: DoksOptions,
    listeners: Listeners,
}

impl<'a> DOKS<'a> {
    pub fn new(
        context: Context,
        id: String,
        name: String,
        version: String,
        region: Region,
        cloud_provider: &'a DO,
        dns_provider: &'a dyn DnsProvider,
        nodes: Vec<Node>,
        options: DoksOptions,
    ) -> Self {
        let template_directory = format!("{}/digitalocean/bootstrap", context.lib_root_dir());

        let spaces = Spaces::new(
            context.clone(),
            "spaces-temp-id".to_string(),
            "my-spaces-object-storage".to_string(),
            cloud_provider.spaces_access_id.clone(),
            cloud_provider.spaces_secret_key.clone(),
            region,
        );

        DOKS {
            context,
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            region,
            cloud_provider,
            dns_provider,
            spaces,
            options,
            nodes,
            template_directory,
            listeners: cloud_provider.listeners.clone(), // copy listeners from CloudProvider
        }
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

    // create a context to render tf files (terraform) contained in lib/digitalocean/
    fn tera_context(&self) -> Result<TeraContext, EngineError> {
        let mut context = TeraContext::new();

        // Digital Ocean
        context.insert("digitalocean_token", &self.cloud_provider.token);
        context.insert("do_region", &self.region.to_string());

        // Digital Ocean: Spaces Credentials
        context.insert("spaces_access_id", &self.cloud_provider.spaces_access_id);
        context.insert("spaces_secret_key", &self.cloud_provider.spaces_secret_key);

        let space_kubeconfig_bucket = format!("qovery-kubeconfigs-{}", self.id.as_str());
        context.insert("space_bucket_kubeconfig", &space_kubeconfig_bucket);

        // Digital Ocean: Network
        context.insert("do_vpc_name", self.options.vpc_name.as_str());
        let vpc_cidr_block = match self.options.vpc_cidr_set {
            // VPC subnet is not set, getting a non used subnet
            VpcInitKind::Autodetect => {
                match get_do_vpc_name_available_from_api(&self.cloud_provider.token, self.options.vpc_name.clone()) {
                    Ok(vpcs) => match vpcs {
                        // new vpc: select a random non used subnet
                        None => {
                            match get_do_random_available_subnet_from_api(&self.cloud_provider.token, self.region) {
                                Ok(x) => x,
                                Err(e) => {
                                    return Err(EngineError {
                                        cause: EngineErrorCause::Internal,
                                        scope: EngineErrorScope::Engine,
                                        execution_id: self.context.execution_id().to_string(),
                                        message: e.message,
                                    })
                                }
                            }
                        }
                        // existing vpc: assign current subnet in this case
                        Some(vpc) => vpc.ip_range,
                    },
                    Err(e) => {
                        return Err(EngineError {
                            cause: EngineErrorCause::Internal,
                            scope: EngineErrorScope::Engine,
                            execution_id: self.context.execution_id().to_string(),
                            message: e.message,
                        })
                    }
                }
            }
            VpcInitKind::Manual => self.options.vpc_cidr_block.clone(),
        };
        context.insert("do_vpc_cidr_block", vpc_cidr_block.as_str());

        // DNS
        let managed_dns_list = vec![self.dns_provider.name()];
        let managed_dns_domains_helm_format = vec![format!("\"{}\"", self.dns_provider.domain())];
        let managed_dns_domains_terraform_format = terraform_list_format(vec![self.dns_provider.domain().to_string()]);
        let managed_dns_resolvers_terraform_format = self.managed_dns_resolvers_terraform_format();

        context.insert("managed_dns", &managed_dns_list);
        context.insert("managed_dns_domain", self.dns_provider.domain());
        context.insert("managed_dns_domains_helm_format", &managed_dns_domains_helm_format);
        context.insert(
            "managed_dns_domains_terraform_format",
            &managed_dns_domains_terraform_format,
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
        let doks_version = match self.get_doks_info_from_name_api() {
            Ok(x) => match x {
                // new cluster, we check the wished version is supported by DO
                None => match get_do_latest_doks_slug_from_api(self.cloud_provider.token.as_str(), self.version()) {
                    Ok(version) => match version {
                        None => return Err(EngineError {
                            cause: EngineErrorCause::Internal,
                            scope: EngineErrorScope::Engine,
                            execution_id: self.context.execution_id().to_string(),
                            message: Some(format!("from the DigitalOcean API, no slug version match the required version ({}). This version is not supported anymore or not yet by DigitalOcean.", self.version()))
                        }),
                        Some(v) => v,
                    }
                    Err(e) => return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: self.context.execution_id().to_string(),
                        message: e.message,
                    })
                },
                // use the same deployed version number
                Some(x) => x.version
            }
            Err(e) => return Err(EngineError {
                cause: EngineErrorCause::Internal,
                scope: EngineErrorScope::Engine,
                execution_id: self.context.execution_id().to_string(),
                message: e.message
            })
        };
        context.insert("doks_version", doks_version.as_str());
        context.insert("do_space_kubeconfig_filename", &self.kubeconfig_file_name());

        // Network
        context.insert("vpc_name", self.options.vpc_name.as_str());
        context.insert("vpc_cidr_block", self.options.vpc_cidr_block.as_str());

        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("object_storage_kubeconfig_bucket", &self.kubeconfig_bucket_name());
        context.insert("object_storage_logs_bucket", &self.logs_bucket_name());

        context.insert(
            "engine_version_controller_token",
            &self.options.engine_version_controller_token,
        );

        context.insert(
            "agent_version_controller_token",
            &self.options.agent_version_controller_token,
        );

        context.insert("test_cluster", &self.context.is_test_cluster());
        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());
        context.insert("qovery_nats_url", self.options.qovery_nats_url.as_str());
        context.insert("qovery_nats_user", self.options.qovery_nats_user.as_str());
        context.insert("qovery_nats_password", self.options.qovery_nats_password.as_str());
        context.insert("qovery_ssh_key", self.options.qovery_ssh_key.as_str());
        context.insert("discord_api_key", self.options.discord_api_key.as_str());

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

        // kubernetes workers
        let worker_nodes = self
            .nodes
            .iter()
            .group_by(|e| e.instance_type())
            .into_iter()
            .map(|(instance_type, group)| (instance_type, group.collect::<Vec<_>>()))
            .map(|(instance_type, nodes)| WorkerNodeDataTemplate {
                instance_type: instance_type.to_string(),
                desired_size: "3".to_string(),
                max_size: nodes.len().to_string(),
                min_size: "3".to_string(),
            })
            .collect::<Vec<WorkerNodeDataTemplate>>();

        context.insert("doks_worker_nodes", &worker_nodes);

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

    // return cluster info from name if exists
    fn get_doks_info_from_name_api(&self) -> Result<Option<KubernetesCluster>, SimpleError> {
        let api_url = format!("{}/clusters", DoApiType::Doks.api_url());
        let json_content = do_get_from_api(self.cloud_provider.token.as_str(), DoApiType::Doks, api_url)?;
        get_doks_info_from_name(json_content.as_str(), self.name().to_string())
    }
}

impl<'a> Kubernetes for DOKS<'a> {
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

    fn region(&self) -> &str {
        self.region.as_str()
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.spaces
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create(&self) -> Result<(), EngineError> {
        info!("DOKS.on_create() called for {}", self.name());

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

        let temp_dir = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("digitalocean/bootstrap/{}", self.name()),
        )
        .map_err(|err| self.engine_error(EngineErrorCause::Internal, err.to_string()))?;

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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/digitialocean/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        // temporary: remove helm/kube management from terraform
        match terraform_init_validate_state_list(temp_dir.as_str()) {
            Ok(x) => {
                let items_type = vec!["helm_release", "kubernetes_namespace"];
                for item in items_type {
                    for entry in x.clone() {
                        if entry.starts_with(item) {
                            match terraform_exec(temp_dir.as_str(), vec!["state", "rm", &entry]) {
                                Ok(_) => info!("successfully removed {}", &entry),
                                Err(e) => {
                                    return Err(EngineError {
                                        cause: EngineErrorCause::Internal,
                                        scope: EngineErrorScope::Engine,
                                        execution_id: self.context.execution_id().to_string(),
                                        message: Some(format!(
                                            "error while trying to remove {} out of terraform state file. {:?}",
                                            entry, e.message
                                        )),
                                    })
                                }
                            }
                        };
                    }
                }
            }
            Err(e) => warn!(
                "no state list exists yet, this is normal if it's a newly created cluster. {:?}",
                e
            ),
        };

        info!("Create Qovery managed object storage buckets");
        // Kubeconfig bucket
        if let Err(e) = self.spaces.create_bucket(self.kubeconfig_bucket_name().as_str()) {
            let message = format!(
                "cannot create object storage bucket {} for cluster {} with id {}",
                self.kubeconfig_bucket_name(),
                self.name(),
                self.id()
            );
            error!("{}", message);
            return Err(e);
        }

        // Logs bucket
        if let Err(e) = self.spaces.create_bucket(self.logs_bucket_name().as_str()) {
            let message = format!(
                "cannot create object storage bucket {} for cluster {} with id {}",
                self.logs_bucket_name(),
                self.name(),
                self.id()
            );
            error!("{}", message);
            return Err(e);
        }

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

        // push config file to object storage
        let kubeconfig_name = format!("{}.yaml", self.id());
        if let Err(e) = self.spaces.put(
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
        // todo: instead of downloading kubeconfig file, use the one that has just been generated by terraform
        let kubeconfig_file = match self.config_file() {
            Ok(x) => x.0,
            Err(e) => {
                error!("kubernetes cluster has just been deployed, but kubeconfig wasn't available, can't finish installation");
                return Err(e);
            }
        };
        let kubeconfig = PathBuf::from(&kubeconfig_file);
        let credentials_environment_variables: Vec<(String, String)> = self
            .cloud_provider
            .credentials_environment_variables()
            .into_iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect();

        let doks_id = match self.get_doks_info_from_name_api() {
            Ok(info) => match info {
                None => {
                    return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: self.context.execution_id().to_string(),
                        message: Some(format!(
                            "DigitalOcean API reported no cluster id, while it has been deployed, please retry later"
                        )),
                    })
                }
                Some(cluster) => cluster.id,
            },
            Err(e) => {
                return Err(EngineError {
                    cause: EngineErrorCause::Internal,
                    scope: EngineErrorScope::Engine,
                    execution_id: self.context.execution_id().to_string(),
                    message: e.message,
                })
            }
        };

        let charts_prerequisites = ChartsConfigPrerequisites {
            organization_id: self.cloud_provider.organization_id().to_string(),
            infra_options: self.options.clone(),
            cluster_id: self.id.clone(),
            do_cluster_id: doks_id,
            region: self.region().to_string(),
            cluster_name: self.cluster_name().to_string(),
            cloud_provider: "digitalocean".to_string(),
            test_cluster: self.context.is_test_cluster(),
            do_token: self.cloud_provider.token.to_string(),
            do_space_access_id: self.cloud_provider.spaces_access_id.to_string(),
            do_space_secret_key: self.cloud_provider.spaces_secret_key.to_string(),
            do_space_bucket_kubeconfig: self.kubeconfig_bucket_name(),
            do_space_kubeconfig_filename: self.kubeconfig_file_name(),
            ff_log_history_enabled: self.context.is_feature_enabled(&Features::LogsHistory),
            ff_metrics_history_enabled: self.context.is_feature_enabled(&Features::MetricsHistory),
            managed_dns_name: self.dns_provider.domain().to_string(),
            managed_dns_helm_format: self.dns_provider.domain_helm_format(),
            managed_dns_resolvers_terraform_format: self.managed_dns_resolvers_terraform_format(),
            external_dns_provider: self.dns_provider.provider_name().to_string(),
            dns_email_report: self.options.tls_email_report.clone(),
            acme_url: self.lets_encrypt_url(),
            cloudflare_email: self.dns_provider.account().to_string(),
            cloudflare_api_token: self.dns_provider.token().to_string(),
            disable_pleco: self.context.disable_pleco(),
        };

        let helm_charts_to_deploy = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            do_helm_charts(
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

    fn on_create_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_upgrade(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_upgrade_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_downgrade(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_downgrade_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause(&self) -> Result<(), EngineError> {
        todo!()
    }

    fn on_pause_error(&self) -> Result<(), EngineError> {
        todo!()
    }

    fn on_delete(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn deploy_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("DOKS.deploy_environment() called for {}", self.name());
        kubernetes::deploy_environment(self, environment)
    }

    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), EngineError> {
        warn!("DOKS.deploy_environment_error() called for {}", self.name());
        kubernetes::deploy_environment_error(self, environment)
    }

    fn pause_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("DOKS.pause_environment() called for {}", self.name());
        kubernetes::pause_environment(self, environment)
    }

    fn pause_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        warn!("DOKS.pause_environment_error() called for {}", self.name());
        Ok(())
    }

    fn delete_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("DOKS.delete_environment() called for {}", self.name());
        kubernetes::delete_environment(self, environment)
    }

    fn delete_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        warn!("DOKS.delete_environment_error() called for {}", self.name());
        Ok(())
    }
}

impl<'a> Listen for DOKS<'a> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
