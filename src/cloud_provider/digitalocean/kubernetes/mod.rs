use std::env;

use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::aws::regions::AwsZones;
use crate::cloud_provider::digitalocean::application::DoRegion;
use crate::cloud_provider::digitalocean::do_api_common::{do_get_from_api, DoApiType};
use crate::cloud_provider::digitalocean::kubernetes::doks_api::{
    get_do_latest_doks_slug_from_api, get_doks_info_from_name,
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
use crate::cmd::helm::{helm_exec_upgrade_with_chart_info, helm_upgrade_diff_with_chart_info};
use crate::cmd::kubectl::{
    do_kubectl_exec_get_loadbalancer_id, kubectl_exec_get_all_namespaces, kubectl_exec_get_events,
};
use crate::cmd::structs::HelmChart;
use crate::cmd::terraform::{terraform_exec, terraform_init_validate_plan_apply, terraform_init_validate_state_list};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::error::EngineErrorCause::Internal;
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope, SimpleError};
use crate::errors::EngineError as NewEngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::logger::Logger;
use crate::models::{
    Action, Context, Features, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel,
    ProgressScope, ToHelmString,
};
use crate::object_storage::spaces::{BucketDeleteStrategy, Spaces};
use crate::object_storage::ObjectStorage;
use crate::string::terraform_list_format;
use crate::{cmd, dns_provider};
use ::function_name::named;
use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::OperationResult;
use std::path::Path;
use std::str::FromStr;

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

pub struct DOKS<'a> {
    context: Context,
    id: String,
    long_id: uuid::Uuid,
    name: String,
    version: String,
    region: DoRegion,
    cloud_provider: &'a dyn CloudProvider,
    nodes_groups: Vec<NodeGroups>,
    dns_provider: &'a dyn DnsProvider,
    spaces: Spaces,
    template_directory: String,
    options: DoksOptions,
    listeners: Listeners,
    logger: &'a dyn Logger,
}

impl<'a> DOKS<'a> {
    pub fn new(
        context: Context,
        id: String,
        long_id: uuid::Uuid,
        name: String,
        version: String,
        region: DoRegion,
        cloud_provider: &'a dyn CloudProvider,
        dns_provider: &'a dyn DnsProvider,
        nodes_groups: Vec<NodeGroups>,
        options: DoksOptions,
        logger: &'a dyn Logger,
    ) -> Result<Self, EngineError> {
        let template_directory = format!("{}/digitalocean/bootstrap", context.lib_root_dir());

        for node_group in &nodes_groups {
            if DoInstancesType::from_str(node_group.instance_type.as_str()).is_err() {
                return Err(EngineError::new(
                    EngineErrorCause::Internal,
                    EngineErrorScope::Engine,
                    context.execution_id(),
                    Some(format!(
                        "Nodegroup instance type {} is not valid for {}",
                        node_group.instance_type,
                        cloud_provider.name()
                    )),
                ));
            }
        }

        let spaces = Spaces::new(
            context.clone(),
            "spaces-temp-id".to_string(),
            "my-spaces-object-storage".to_string(),
            cloud_provider.access_key_id().clone(),
            cloud_provider.secret_access_key().clone(),
            region,
            BucketDeleteStrategy::HardDelete,
        );

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
            listeners: cloud_provider.listeners().clone(), // copy listeners from CloudProvider
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

    // create a context to render tf files (terraform) contained in lib/digitalocean/
    fn tera_context(&self) -> Result<TeraContext, EngineError> {
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
                            match get_do_random_available_subnet_from_api(&self.cloud_provider.token(), self.region) {
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

        // DOKS
        context.insert("test_cluster", &self.context.is_test_cluster());
        context.insert("doks_cluster_id", &self.id());
        context.insert("doks_master_name", &self.name());
        let doks_version = match self.get_doks_info_from_name_api() {
            Ok(x) => match x {
                // new cluster, we check the wished version is supported by DO
                None => match get_do_latest_doks_slug_from_api(self.cloud_provider.token(), self.version()) {
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
        context.insert("doks_worker_nodes", &self.nodes_groups);

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

    fn do_loadbalancer_hostname(&self) -> String {
        format!(
            "qovery-nginx-{}.{}",
            self.cloud_provider.id(),
            self.dns_provider().domain()
        )
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
        let json_content = do_get_from_api(self.cloud_provider.token(), DoApiType::Doks, api_url)?;
        // TODO(benjaminch): `qovery-` to be added into Rust name directly everywhere
        get_doks_info_from_name(json_content.as_str(), format!("qovery-{}", self.id().to_string()))
    }

    fn create(&self) -> Result<(), EngineError> {
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

        // upgrade cluster instead if required
        match self.get_kubeconfig_file_path() {
            Ok(p) => match is_kubernetes_upgrade_required(
                p.as_str(),
                &self.version,
                self.cloud_provider.credentials_environment_variables(),
            ) {
                Ok(x) => {
                    if x.required_upgrade_on.is_some() {
                        return self.upgrade_with_status(x);
                    }
                    info!("Kubernetes cluster upgrade not required");
                }
                Err(e) => error!(
                    "Error detected, upgrade won't occurs, but standard deployment. {:?}",
                    e.message
                ),
            },
            Err(_) => {
                info!("Kubernetes cluster upgrade not required, config file is not found and cluster have certainly never been deployed before");
            }
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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/digitalocean/bootstrap/*.tf files
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
                                            "error while trying to remove {} out of terraform state file.\n {:?}",
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
        let kubeconfig_path = match self.get_kubeconfig_file_path() {
            Ok(path) => path,
            Err(e) => {
                error!("kubernetes cluster has just been deployed, but kubeconfig wasn't available, can't finish installation");
                return Err(e.to_legacy_engine_error());
            }
        };
        let kubeconfig = Path::new(&kubeconfig_path);
        let credentials_environment_variables: Vec<(String, String)> = self
            .cloud_provider
            .credentials_environment_variables()
            .into_iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect();

        let doks_id =
            match self.get_doks_info_from_name_api() {
                Ok(info) => match info {
                    None => return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: self.context.execution_id().to_string(),
                        message: Some(
                            "DigitalOcean API reported no cluster id, while it has been deployed, please retry later"
                                .to_string(),
                        ),
                    }),
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
            organization_long_id: self.cloud_provider.organization_long_id(),
            infra_options: self.options.clone(),
            cluster_id: self.id.clone(),
            cluster_long_id: self.long_id,
            do_cluster_id: doks_id,
            region: self.region().to_string(),
            cluster_name: self.cluster_name().to_string(),
            cloud_provider: "digitalocean".to_string(),
            test_cluster: self.context.is_test_cluster(),
            do_token: self.cloud_provider.token().to_string(),
            do_space_access_id: self.cloud_provider.access_key_id().to_string(),
            do_space_secret_key: self.cloud_provider.secret_access_key().to_string(),
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

        let helm_charts_to_deploy = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            do_helm_charts(
                format!("{}/qovery-tf-config.json", &temp_dir).as_str(),
                &charts_prerequisites,
                Some(chart_prefix_path),
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
        )?;

        // https://github.com/digitalocean/digitalocean-cloud-controller-manager/blob/master/docs/controllers/services/annotations.md#servicebetakubernetesiodo-loadbalancer-hostname
        // it can't be done earlier as nginx ingress is not yet deployed
        // required as load balancer do not have hostname (only IP) and are blocker to get a TLS certificate
        let nginx_ingress_loadbalancer_id = match do_kubectl_exec_get_loadbalancer_id(
                &kubeconfig,
                "nginx-ingress",
                "nginx-ingress-ingress-nginx-controller",
                self.cloud_provider.credentials_environment_variables(),
            ) {
                Ok(x) => match x {
                    None => return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: self.context.execution_id().to_string(),
                        message: Some("No associated Load balancer UUID was found on DigitalOcean API and it's required for TLS setup.".to_string())
                    }),
                    Some(uuid) => uuid,
                },
                Err(e) => {
                    return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: self.context.execution_id().to_string(),
                        message: Some(format!(
                            "Load balancer IP wasn't able to be retrieved and it's required for TLS setup. {:?}",
                            e.message
                        )),
                    })
                }
            };
        let nginx_ingress_loadbalancer_ip = match do_get_load_balancer_ip(self.cloud_provider.token(), nginx_ingress_loadbalancer_id.as_str()) {
                Ok(x) => x.to_string(),
                Err(e) => {
                    return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: self.context.execution_id().to_string(),
                        message: Some(format!(
                            "Load balancer IP wasn't able to be retrieved from UUID on DigitalOcean API and it's required for TLS setup. {:?}",
                            e.message
                        )),
                    })
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

        let _ = helm_upgrade_diff_with_chart_info(
            &kubeconfig,
            &credentials_environment_variables,
            &load_balancer_dns_hostname,
        );

        match helm_exec_upgrade_with_chart_info(
            &kubeconfig,
            &self.cloud_provider.credentials_environment_variables(),
            &load_balancer_dns_hostname,
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(EngineError {
                cause: EngineErrorCause::Internal,
                scope: EngineErrorScope::Engine,
                execution_id: self.context.execution_id().to_string(),
                message: Some(format!(
                    "Error while deploying chart {}. {:?}",
                    load_balancer_dns_hostname.name, e.message
                )),
            }),
        }
    }

    fn create_error(&self) -> Result<(), EngineError> {
        let kubeconfig = match self.get_kubeconfig_file() {
            Ok((path, _)) => path,
            Err(e) => {
                error!("kubernetes cluster has just been deployed, but kubeconfig wasn't available, can't finish installation");
                return Err(e.to_legacy_engine_error());
            }
        };
        let environment_variables: Vec<(&str, &str)> = self.cloud_provider.credentials_environment_variables();
        warn!("DOKS.create_error() called for {}", self.name());
        match kubectl_exec_get_events(kubeconfig, None, environment_variables) {
            Ok(ok_line) => info!("{}", ok_line),
            Err(err) => error!("{:?}", err),
        };
        Err(self.engine_error(
            EngineErrorCause::Internal,
            format!("{} Kubernetes cluster failed on deployment", self.name()),
        ))
    }

    fn upgrade_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn downgrade(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn downgrade_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn pause(&self) -> Result<(), EngineError> {
        todo!()
    }

    fn pause_error(&self) -> Result<(), EngineError> {
        todo!()
    }

    fn delete(&self) -> Result<(), EngineError> {
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let mut skip_kubernetes_step = false;
        self.send_to_customer(
            format!("Preparing to delete DOKS cluster {} with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        let temp_dir = match self.get_temp_dir() {
            Ok(dir) => dir,
            Err(e) => return Err(e),
        };

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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/digitalocean/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/digital/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        let kubernetes_config_file_path = match self.get_kubeconfig_file_path() {
            Ok(x) => x,
            Err(e) => {
                warn!(
                    "skipping Kubernetes uninstall because it can't be reached. {:?}",
                    e.message(),
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
                return Err(EngineError::new(
                    Internal,
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
                return Err(EngineError::new(
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

        info!("Empty Qovery managed object storage buckets");
        if let Err(e) = self.spaces.empty_bucket(self.kubeconfig_bucket_name().as_str()) {
            return Err(EngineError::new(
                EngineErrorCause::Internal,
                self.engine_error_scope(),
                self.context().execution_id(),
                e.message,
            ));
        }

        Ok(())
    }

    fn delete_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn cloud_provider_name(&self) -> &str {
        "digitalocean"
    }

    fn struct_name(&self) -> &str {
        "kubernetes"
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
        self.cloud_provider
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider
    }

    fn logger(&self) -> &dyn Logger {
        self.logger
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.spaces
    }

    fn is_valid(&self) -> Result<(), NewEngineError> {
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

    fn upgrade_with_status(&self, _kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), EngineError> {
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

        let temp_dir = match self.get_temp_dir() {
            Ok(dir) => dir,
            Err(e) => return Err(e),
        };

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        match self.delete_crashlooping_pods(
            None,
            None,
            Some(10),
            self.cloud_provider().credentials_environment_variables(),
        ) {
            Ok(..) => {}
            Err(e) => {
                error!(
                    "Error while upgrading nodes for cluster {} with id {}. {}",
                    self.name(),
                    self.id(),
                    e.message.clone().unwrap_or("Can't get error message".to_string()),
                );
                return Err(e);
            }
        };

        //
        // Upgrade nodes
        //
        let message = format!("Start upgrading process for nodes on {}/{}", self.name(), self.id());
        info!("{}", &message);
        self.send_to_customer(&message, &listeners_helper);

        let upgrade_doks_version =  match get_do_latest_doks_slug_from_api(self.cloud_provider.token(), self.version()) {
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
                };

        context.insert("doks_version", format!("{}", &upgrade_doks_version).as_str());

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
            Ok(_) => match self.check_workers_on_upgrade(upgrade_doks_version) {
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
                    return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: self.context.execution_id().to_string(),
                        message: e.message,
                    });
                }
            },
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
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::deploy_environment_error(self, environment)
    }

    #[named]
    fn pause_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::pause_environment(self, environment)
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
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        kubernetes::delete_environment(self, environment)
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

impl<'a> Listen for DOKS<'a> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
