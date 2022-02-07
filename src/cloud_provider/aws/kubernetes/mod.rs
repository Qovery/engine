use core::fmt;
use std::env;
use std::path::Path;
use std::str::FromStr;

use retry::delay::{Fibonacci, Fixed};
use retry::Error::Operation;
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::aws::kubernetes::helm_charts::{aws_helm_charts, ChartsConfigPrerequisites};
use crate::cloud_provider::aws::kubernetes::node::AwsInstancesType;
use crate::cloud_provider::aws::kubernetes::roles::get_default_roles_to_create;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::helm::deploy_charts_levels;
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, send_progress_on_long_task, uninstall_cert_manager, Kind, Kubernetes,
    KubernetesNodesType, KubernetesUpgradeStatus, ProviderOptions,
};
use crate::cloud_provider::models::{NodeGroups, NodeGroupsFormat};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd;
use crate::cmd::kubectl::{
    kubectl_exec_api_custom_metrics, kubectl_exec_get_all_namespaces, kubectl_exec_get_events,
    kubectl_exec_scale_replicas, ScalingKind,
};
use crate::cmd::structs::HelmChart;
use crate::cmd::terraform::{terraform_exec, terraform_init_validate_plan_apply, terraform_init_validate_state_list};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider;
use crate::dns_provider::DnsProvider;
use crate::error::EngineErrorCause::Internal;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope, SimpleError, SimpleErrorKind,
};
use crate::errors::EngineError as NewEngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::logger::Logger;
use crate::models::{
    Action, Context, Features, Listen, Listener, Listeners, ListenersHelper, ToHelmString, ToTerraformString,
};
use crate::object_storage::s3::S3;
use crate::object_storage::ObjectStorage;
use crate::string::terraform_list_format;
use ::function_name::named;

pub mod helm_charts;
pub mod node;
pub mod roles;

// https://docs.aws.amazon.com/eks/latest/userguide/external-snat.html
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VpcQoveryNetworkMode {
    WithNatGateways,
    WithoutNatGateways,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpcCustomRoutingTable {
    description: String,
    destination: String,
    target: String,
}

impl fmt::Display for VpcQoveryNetworkMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    // AWS related
    pub eks_zone_a_subnet_blocks: Vec<String>,
    pub eks_zone_b_subnet_blocks: Vec<String>,
    pub eks_zone_c_subnet_blocks: Vec<String>,
    pub rds_zone_a_subnet_blocks: Vec<String>,
    pub rds_zone_b_subnet_blocks: Vec<String>,
    pub rds_zone_c_subnet_blocks: Vec<String>,
    pub documentdb_zone_a_subnet_blocks: Vec<String>,
    pub documentdb_zone_b_subnet_blocks: Vec<String>,
    pub documentdb_zone_c_subnet_blocks: Vec<String>,
    pub elasticache_zone_a_subnet_blocks: Vec<String>,
    pub elasticache_zone_b_subnet_blocks: Vec<String>,
    pub elasticache_zone_c_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_a_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_b_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_c_subnet_blocks: Vec<String>,
    pub vpc_qovery_network_mode: VpcQoveryNetworkMode,
    pub vpc_cidr_block: String,
    pub eks_cidr_subnet: String,
    pub vpc_custom_routing_table: Vec<VpcCustomRoutingTable>,
    pub eks_access_cidr_blocks: Vec<String>,
    pub rds_cidr_subnet: String,
    pub documentdb_cidr_subnet: String,
    pub elasticache_cidr_subnet: String,
    pub elasticsearch_cidr_subnet: String,
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

impl ProviderOptions for Options {}

pub struct EKS<'a> {
    context: Context,
    id: String,
    long_id: uuid::Uuid,
    name: String,
    version: String,
    region: AwsRegion,
    zones: Vec<AwsZones>,
    cloud_provider: &'a dyn CloudProvider,
    dns_provider: &'a dyn DnsProvider,
    s3: S3,
    nodes_groups: Vec<NodeGroups>,
    template_directory: String,
    options: Options,
    listeners: Listeners,
    logger: &'a dyn Logger,
}

impl<'a> EKS<'a> {
    pub fn new(
        context: Context,
        id: &str,
        long_id: uuid::Uuid,
        name: &str,
        version: &str,
        region: AwsRegion,
        zones: Vec<String>,
        cloud_provider: &'a dyn CloudProvider,
        dns_provider: &'a dyn DnsProvider,
        options: Options,
        nodes_groups: Vec<NodeGroups>,
        logger: &'a dyn Logger,
    ) -> Result<Self, EngineError> {
        let template_directory = format!("{}/aws/bootstrap", context.lib_root_dir());

        let mut aws_zones: Vec<AwsZones> = Vec::with_capacity(3);
        for zone in zones {
            match AwsZones::from_string(zone.to_string()) {
                Ok(x) => aws_zones.push(x),
                Err(e) => {
                    return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: id.to_string(),
                        message: Some(format!("Zone may not be found or supported: {:?}", e)),
                    })
                }
            };
        }

        for node_group in &nodes_groups {
            if AwsInstancesType::from_str(node_group.instance_type.as_str()).is_err() {
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

        // TODO export this
        let s3 = S3::new(
            context.clone(),
            "s3-temp-id".to_string(),
            "default-s3".to_string(),
            cloud_provider.access_key_id().clone(),
            cloud_provider.secret_access_key().clone(),
        );

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
            listeners: cloud_provider.listeners().clone(), // copy listeners from CloudProvider
        })
    }

    fn get_engine_location(&self) -> EngineLocation {
        self.options.qovery_engine_location.clone()
    }

    fn kubeconfig_bucket_name(&self) -> String {
        format!("qovery-kubeconfigs-{}", self.id())
    }

    fn managed_dns_resolvers_terraform_format(&self) -> String {
        let managed_dns_resolvers: Vec<String> = self
            .dns_provider
            .resolvers()
            .iter()
            .map(|x| format!("{}", x.clone().to_string()))
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

    // divide by 2 the total number of subnet to get the exact same number as private and public
    fn check_odd_subnets(&self, zone_name: &str, subnet_block: &Vec<String>) -> Result<usize, EngineError> {
        let is_odd = subnet_block.len() % 2;

        if is_odd == 1 {
            Err(EngineError {
                cause: EngineErrorCause::Internal,
                scope: EngineErrorScope::Engine,
                execution_id: self.context.execution_id().to_string(),
                message: Some(format!(
                    "the number of subnets for zone '{}' should be an even number, not an odd!",
                    zone_name
                )),
            })
        } else {
            Ok((subnet_block.len() / 2) as usize)
        }
    }

    fn tera_context(&self) -> Result<TeraContext, EngineError> {
        let mut context = TeraContext::new();

        let format_ips =
            |ips: &Vec<String>| -> Vec<String> { ips.iter().map(|ip| format!("\"{}\"", ip)).collect::<Vec<_>>() };
        let format_zones = |zones: &Vec<AwsZones>| -> Vec<String> {
            zones
                .iter()
                .map(|zone| zone.to_terraform_format_string())
                .collect::<Vec<_>>()
        };

        let aws_zones = format_zones(&self.zones);

        let mut eks_zone_a_subnet_blocks_private = format_ips(&self.options.eks_zone_a_subnet_blocks);
        let mut eks_zone_b_subnet_blocks_private = format_ips(&self.options.eks_zone_b_subnet_blocks);
        let mut eks_zone_c_subnet_blocks_private = format_ips(&self.options.eks_zone_c_subnet_blocks);

        match self.options.vpc_qovery_network_mode {
            VpcQoveryNetworkMode::WithNatGateways => {
                let max_subnet_zone_a = self.check_odd_subnets("a", &eks_zone_a_subnet_blocks_private)?;
                let max_subnet_zone_b = self.check_odd_subnets("b", &eks_zone_b_subnet_blocks_private)?;
                let max_subnet_zone_c = self.check_odd_subnets("c", &eks_zone_c_subnet_blocks_private)?;

                let eks_zone_a_subnet_blocks_public: Vec<String> =
                    eks_zone_a_subnet_blocks_private.drain(max_subnet_zone_a..).collect();
                let eks_zone_b_subnet_blocks_public: Vec<String> =
                    eks_zone_b_subnet_blocks_private.drain(max_subnet_zone_b..).collect();
                let eks_zone_c_subnet_blocks_public: Vec<String> =
                    eks_zone_c_subnet_blocks_private.drain(max_subnet_zone_c..).collect();

                context.insert("eks_zone_a_subnet_blocks_public", &eks_zone_a_subnet_blocks_public);
                context.insert("eks_zone_b_subnet_blocks_public", &eks_zone_b_subnet_blocks_public);
                context.insert("eks_zone_c_subnet_blocks_public", &eks_zone_c_subnet_blocks_public);
            }
            VpcQoveryNetworkMode::WithoutNatGateways => {}
        };
        context.insert(
            "vpc_qovery_network_mode",
            &self.options.vpc_qovery_network_mode.to_string(),
        );

        let rds_zone_a_subnet_blocks = format_ips(&self.options.rds_zone_a_subnet_blocks);
        let rds_zone_b_subnet_blocks = format_ips(&self.options.rds_zone_b_subnet_blocks);
        let rds_zone_c_subnet_blocks = format_ips(&self.options.rds_zone_c_subnet_blocks);

        let documentdb_zone_a_subnet_blocks = format_ips(&self.options.documentdb_zone_a_subnet_blocks);
        let documentdb_zone_b_subnet_blocks = format_ips(&self.options.documentdb_zone_b_subnet_blocks);
        let documentdb_zone_c_subnet_blocks = format_ips(&self.options.documentdb_zone_c_subnet_blocks);

        let elasticache_zone_a_subnet_blocks = format_ips(&self.options.elasticache_zone_a_subnet_blocks);
        let elasticache_zone_b_subnet_blocks = format_ips(&self.options.elasticache_zone_b_subnet_blocks);
        let elasticache_zone_c_subnet_blocks = format_ips(&self.options.elasticache_zone_c_subnet_blocks);

        let elasticsearch_zone_a_subnet_blocks = format_ips(&self.options.elasticsearch_zone_a_subnet_blocks);
        let elasticsearch_zone_b_subnet_blocks = format_ips(&self.options.elasticsearch_zone_b_subnet_blocks);
        let elasticsearch_zone_c_subnet_blocks = format_ips(&self.options.elasticsearch_zone_c_subnet_blocks);

        let region_cluster_id = format!("{}-{}", self.region(), self.id());
        let vpc_cidr_block = self.options.vpc_cidr_block.clone();
        let eks_cloudwatch_log_group = format!("/aws/eks/{}/cluster", self.id());
        let eks_cidr_subnet = self.options.eks_cidr_subnet.clone();

        let eks_access_cidr_blocks = format_ips(&self.options.eks_access_cidr_blocks);

        let qovery_api_url = self.options.qovery_api_url.clone();
        let rds_cidr_subnet = self.options.rds_cidr_subnet.clone();
        let documentdb_cidr_subnet = self.options.documentdb_cidr_subnet.clone();
        let elasticache_cidr_subnet = self.options.elasticache_cidr_subnet.clone();
        let elasticsearch_cidr_subnet = self.options.elasticsearch_cidr_subnet.clone();

        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("qovery_api_url", &qovery_api_url);

        context.insert(
            "engine_version_controller_token",
            &self.options.engine_version_controller_token,
        );
        context.insert(
            "agent_version_controller_token",
            &self.options.agent_version_controller_token,
        );

        context.insert("test_cluster", &self.context.is_test_cluster());
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }
        context.insert("force_upgrade", &self.context.requires_forced_upgrade());

        // Qovery features
        context.insert(
            "log_history_enabled",
            &self.context.is_feature_enabled(&Features::LogsHistory),
        );
        context.insert(
            "metrics_history_enabled",
            &self.context.is_feature_enabled(&Features::MetricsHistory),
        );

        // DNS configuration
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

        // TLS
        context.insert("acme_server_url", &self.lets_encrypt_url());

        // Vault
        context.insert("vault_auth_method", "none");

        if let Some(_) = env::var_os("VAULT_ADDR") {
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
                    if let Some(_) = env::var_os("VAULT_TOKEN") {
                        context.insert("vault_auth_method", "token")
                    }
                }
            }
        };

        // Other Kubernetes
        context.insert("kubernetes_cluster_name", &self.cluster_name());
        context.insert("enable_cluster_autoscaler", &true);

        // AWS
        context.insert("aws_access_key", &self.cloud_provider.access_key_id());
        context.insert("aws_secret_key", &self.cloud_provider.secret_access_key());

        // AWS S3 tfstate storage
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

        context.insert("aws_region", &self.region());
        context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");
        context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");
        context.insert("vpc_cidr_block", &vpc_cidr_block);
        context.insert("vpc_custom_routing_table", &self.options.vpc_custom_routing_table);
        context.insert("s3_kubeconfig_bucket", &self.kubeconfig_bucket_name());

        // AWS - EKS
        context.insert("aws_availability_zones", &aws_zones);
        context.insert("eks_cidr_subnet", &eks_cidr_subnet.clone());
        context.insert("kubernetes_cluster_name", &self.name());
        context.insert("kubernetes_cluster_id", self.id());
        context.insert("eks_region_cluster_id", region_cluster_id.as_str());
        context.insert("eks_worker_nodes", &self.nodes_groups);
        context.insert("eks_zone_a_subnet_blocks_private", &eks_zone_a_subnet_blocks_private);
        context.insert("eks_zone_b_subnet_blocks_private", &eks_zone_b_subnet_blocks_private);
        context.insert("eks_zone_c_subnet_blocks_private", &eks_zone_c_subnet_blocks_private);
        context.insert("eks_masters_version", &self.version());
        context.insert("eks_workers_version", &self.version());
        context.insert("eks_cloudwatch_log_group", &eks_cloudwatch_log_group);
        context.insert("eks_access_cidr_blocks", &eks_access_cidr_blocks);

        // AWS - RDS
        context.insert("rds_cidr_subnet", &rds_cidr_subnet);
        context.insert("rds_zone_a_subnet_blocks", &rds_zone_a_subnet_blocks);
        context.insert("rds_zone_b_subnet_blocks", &rds_zone_b_subnet_blocks);
        context.insert("rds_zone_c_subnet_blocks", &rds_zone_c_subnet_blocks);

        // AWS - DocumentDB
        context.insert("documentdb_cidr_subnet", &documentdb_cidr_subnet);
        context.insert("documentdb_zone_a_subnet_blocks", &documentdb_zone_a_subnet_blocks);
        context.insert("documentdb_zone_b_subnet_blocks", &documentdb_zone_b_subnet_blocks);
        context.insert("documentdb_zone_c_subnet_blocks", &documentdb_zone_c_subnet_blocks);

        // AWS - Elasticache
        context.insert("elasticache_cidr_subnet", &elasticache_cidr_subnet);
        context.insert("elasticache_zone_a_subnet_blocks", &elasticache_zone_a_subnet_blocks);
        context.insert("elasticache_zone_b_subnet_blocks", &elasticache_zone_b_subnet_blocks);
        context.insert("elasticache_zone_c_subnet_blocks", &elasticache_zone_c_subnet_blocks);

        // AWS - Elasticsearch
        context.insert("elasticsearch_cidr_subnet", &elasticsearch_cidr_subnet);
        context.insert(
            "elasticsearch_zone_a_subnet_blocks",
            &elasticsearch_zone_a_subnet_blocks,
        );
        context.insert(
            "elasticsearch_zone_b_subnet_blocks",
            &elasticsearch_zone_b_subnet_blocks,
        );
        context.insert(
            "elasticsearch_zone_c_subnet_blocks",
            &elasticsearch_zone_c_subnet_blocks,
        );

        // grafana credentials
        context.insert("grafana_admin_user", self.options.grafana_admin_user.as_str());
        context.insert("grafana_admin_password", self.options.grafana_admin_password.as_str());

        // qovery
        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());
        context.insert("qovery_nats_url", self.options.qovery_nats_url.as_str());
        context.insert("qovery_nats_user", self.options.qovery_nats_user.as_str());
        context.insert("qovery_nats_password", self.options.qovery_nats_password.as_str());
        context.insert("qovery_ssh_key", self.options.qovery_ssh_key.as_str());
        context.insert("discord_api_key", self.options.discord_api_key.as_str());

        Ok(context)
    }

    fn create(&self) -> Result<(), EngineError> {
        let listeners_helper = ListenersHelper::new(&self.listeners);

        self.send_to_customer(
            format!("Preparing EKS {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        // upgrade cluster instead if required
        match self.get_kubeconfig_file() {
            Ok(f) => match is_kubernetes_upgrade_required(
                f.0,
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

        // create AWS IAM roles
        let already_created_roles = get_default_roles_to_create();
        for role in already_created_roles {
            match role.create_service_linked_role(
                self.cloud_provider.access_key_id().as_str(),
                self.cloud_provider.secret_access_key().as_str(),
            ) {
                Ok(_) => info!("Role {} is already present, no need to create", role.role_name),
                Err(e) => error!(
                    "Error while getting, or creating the role {} : causing by {:?}",
                    role.role_name, e
                ),
            }
        }

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
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
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
            format!("Deploying EKS {} cluster deployment with id {}", self.name(), self.id()).as_str(),
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

        // kubernetes helm deployments on the cluster
        // todo: instead of downloading kubeconfig file, use the one that has just been generated by terraform
        let kubeconfig_path = match self.get_kubeconfig_file() {
            Ok((path, _)) => path,
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
        let charts_prerequisites = ChartsConfigPrerequisites {
            organization_id: self.cloud_provider.organization_id().to_string(),
            organization_long_id: self.cloud_provider.organization_long_id(),
            infra_options: self.options.clone(),
            cluster_id: self.id.clone(),
            cluster_long_id: self.long_id,
            region: self.region(),
            cluster_name: self.cluster_name().to_string(),
            cloud_provider: "aws".to_string(),
            test_cluster: self.context.is_test_cluster(),
            aws_access_key_id: self.cloud_provider.access_key_id().to_string(),
            aws_secret_access_key: self.cloud_provider.secret_access_key().to_string(),
            vpc_qovery_network_mode: self.options.vpc_qovery_network_mode.clone(),
            qovery_engine_location: self.get_engine_location(),
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

        let helm_charts_to_deploy = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            aws_helm_charts(
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

    fn create_error(&self) -> Result<(), EngineError> {
        let kubeconfig = match self.get_kubeconfig_file() {
            Ok((path, _)) => path,
            Err(e) => {
                error!("kubernetes cluster has just been deployed, but kubeconfig wasn't available, can't finish installation");
                return Err(e.to_legacy_engine_error());
            }
        };
        let environment_variables: Vec<(&str, &str)> = self.cloud_provider.credentials_environment_variables();
        warn!("EKS.create_error() called for {}", self.name());
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
        let listeners_helper = ListenersHelper::new(&self.listeners);
        self.send_to_customer(
            format!("Preparing EKS {} cluster pause with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        let temp_dir = self.get_temp_dir()?;

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
        let worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
        context.insert("eks_worker_nodes", &worker_nodes);

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
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
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
                    if name.starts_with("aws_eks_node_group.") {
                        tf_workers_resources_name.push(name);
                    }
                }
                tf_workers_resources_name
            }
            Err(e) => {
                return Err(EngineError {
                    cause: EngineErrorCause::Internal,
                    scope: EngineErrorScope::Kubernetes(self.id.clone(), self.name.clone()),
                    execution_id: self.context.execution_id().to_string(),
                    message: e.message,
                })
            }
        };
        if tf_workers_resources.is_empty() {
            return Err(EngineError {
                    cause: EngineErrorCause::Internal,
                    scope: EngineErrorScope::Kubernetes(self.id.clone(), self.name.clone()),
                    execution_id: self.context.execution_id().to_string(),
                    message: Some("No worker nodes present, can't Pause the infrastructure. This can happen if there where a manual operations on the workers or the infrastructure is already pause.".to_string()),
                });
        }

        let kubernetes_config_file_path = match self.get_kubeconfig_file_path() {
            Ok(path) => path,
            Err(e) => {
                return Err(e.to_legacy_engine_error());
            }
        };

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
                            return Err(EngineError {
                                cause: EngineErrorCause::Internal,
                                scope: EngineErrorScope::Engine,
                                execution_id: self.context.execution_id().to_string(),
                                message: error.message,
                            })
                        }
                        Err(retry::Error::Internal(msg)) => {
                            return Err(EngineError::new(
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

        let message = format!("Pausing EKS {} cluster deployment with id {}", self.name(), self.id());
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

    fn pause_error(&self) -> Result<(), EngineError> {
        Err(self.engine_error(
            EngineErrorCause::Internal,
            format!("{} Kubernetes cluster failed to pause", self.name()),
        ))
    }

    fn delete(&self) -> Result<(), EngineError> {
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let mut skip_kubernetes_step = false;
        self.send_to_customer(
            format!("Preparing to delete EKS cluster {} with id {}", self.name(), self.id()).as_str(),
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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
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
            match uninstall_cert_manager(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
            ) {
                Ok(_) => {}
                Err(e) => {
                    return Err(EngineError::new(
                        Internal,
                        self.engine_error_scope(),
                        self.context().execution_id(),
                        e.message,
                    ))
                }
            };

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
                Ok(())
            }
            Err(Operation { error, .. }) => Err(error),
            Err(retry::Error::Internal(msg)) => Err(EngineError::new(
                EngineErrorCause::Internal,
                self.engine_error_scope(),
                self.context().execution_id(),
                Some(format!(
                    "Error while deleting cluster {} with id {}: {}",
                    self.name(),
                    self.id(),
                    msg
                )),
            )),
        }
    }

    fn delete_error(&self) -> Result<(), EngineError> {
        // FIXME What should we do if something goes wrong while deleting the cluster?
        Ok(())
    }

    fn cloud_provider_name(&self) -> &str {
        "aws"
    }

    fn struct_name(&self) -> &str {
        "kubernetes"
    }
}

impl<'a> Kubernetes for EKS<'a> {
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
        self.cloud_provider
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider
    }

    fn logger(&self) -> &dyn Logger {
        self.logger
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.s3
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

    fn upgrade_with_status(&self, kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), EngineError> {
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

        let temp_dir = match self.get_temp_dir() {
            Ok(dir) => dir,
            Err(e) => return Err(e),
        };

        let kubeconfig = match self.get_kubeconfig_file() {
            Ok((path, _)) => path,
            Err(e) => return Err(e.to_legacy_engine_error()),
        };

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        //
        // Upgrade master nodes
        //
        match &kubernetes_upgrade_status.required_upgrade_on {
            Some(KubernetesNodesType::Masters) => {
                let message = format!(
                    "Start upgrading process for master nodes on {}/{}",
                    self.name(),
                    self.id()
                );
                info!("{}", &message);
                self.send_to_customer(&message, &listeners_helper);

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
                    format!("Upgrading Kubernetes {} master nodes", self.name()).as_str(),
                    &listeners_helper,
                );

                match cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()),
                ) {
                    Ok(_) => {
                        let message = format!(
                            "Kubernetes {} master nodes have been successfully upgraded",
                            self.name()
                        );
                        info!("{}", &message);
                        self.send_to_customer(&message, &listeners_helper);
                    }
                    Err(e) => {
                        error!(
                            "Error while upgrading master nodes for cluster {} with id {}.",
                            self.name(),
                            self.id()
                        );
                        return Err(e);
                    }
                }
            }
            Some(KubernetesNodesType::Workers) => {
                info!("No need to perform Kubernetes master upgrade, they are already up to date")
            }
            None => {
                info!("No Kubernetes upgrade required, masters and workers are already up to date");
                return Ok(());
            }
        }

        if let Err(e) = self.delete_crashlooping_pods(
            None,
            None,
            Some(3),
            self.cloud_provider().credentials_environment_variables(),
        ) {
            error!(
                "Error while upgrading nodes for cluster {} with id {}. {}",
                self.name(),
                self.id(),
                e.message.clone().unwrap_or("Can't get error message".to_string()),
            );
            return Err(e);
        };

        //
        // Upgrade worker nodes
        //
        let message = format!(
            "Preparing workers nodes for upgrade for Kubernetes cluster {}",
            self.name()
        );
        info!("{}", &message);
        self.send_to_customer(message.as_str(), &listeners_helper);

        // disable cluster autoscaler to avoid interfering with AWS upgrade procedure
        context.insert("enable_cluster_autoscaler", &false);
        context.insert(
            "eks_workers_version",
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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        let message = format!("Upgrading Kubernetes {} worker nodes", self.name());
        info!("{}", &message);
        self.send_to_customer(message.as_str(), &listeners_helper);

        // disable cluster autoscaler deployment
        info!("down-scaling cluster autoscaler to 0");
        match kubectl_exec_scale_replicas(
            &kubeconfig,
            self.cloud_provider().credentials_environment_variables(),
            "kube-system",
            ScalingKind::Deployment,
            "cluster-autoscaler-aws-cluster-autoscaler",
            0,
        ) {
            Ok(_) => {}
            Err(e) => {
                return Err(EngineError {
                    cause: EngineErrorCause::Internal,
                    scope: EngineErrorScope::Engine,
                    execution_id: self.context.execution_id().to_string(),
                    message: e.message,
                })
            }
        };

        match cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()),
        ) {
            Ok(_) => {
                let message = format!(
                    "Kubernetes {} workers nodes have been successfully upgraded",
                    self.name()
                );
                info!("{}", &message);
                self.send_to_customer(&message, &listeners_helper);
            }
            Err(e) => {
                // enable cluster autoscaler deployment
                info!("up-scaling cluster autoscaler to 1");
                let _ = kubectl_exec_scale_replicas(
                    &kubeconfig,
                    self.cloud_provider().credentials_environment_variables(),
                    "kube-system",
                    ScalingKind::Deployment,
                    "cluster-autoscaler-aws-cluster-autoscaler",
                    1,
                );
                error!(
                    "Error while upgrading master nodes for cluster {} with id {}.",
                    self.name(),
                    self.id()
                );
                return Err(e);
            }
        }

        // enable cluster autoscaler deployment
        info!("up-scaling cluster autoscaler to 1");
        match kubectl_exec_scale_replicas(
            &kubeconfig,
            self.cloud_provider().credentials_environment_variables(),
            "kube-system",
            ScalingKind::Deployment,
            "cluster-autoscaler-aws-cluster-autoscaler",
            1,
        ) {
            Ok(_) => {}
            Err(e) => {
                return Err(EngineError {
                    cause: EngineErrorCause::Internal,
                    scope: EngineErrorScope::Engine,
                    execution_id: self.context.execution_id().to_string(),
                    message: e.message,
                })
            }
        };

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

impl<'a> Listen for EKS<'a> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
