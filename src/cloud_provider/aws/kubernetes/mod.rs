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
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
use crate::logger::{LogLevel, Logger};
use crate::models::{
    Action, Context, Features, Listen, Listener, Listeners, ListenersHelper, QoveryIdentifier, ToHelmString,
    ToTerraformString,
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
        let event_details = EventDetails::new(
            Some(cloud_provider.kind()),
            QoveryIdentifier::new(context.organization_id().to_string()),
            QoveryIdentifier::new(context.cluster_id().to_string()),
            QoveryIdentifier::new(context.execution_id().to_string()),
            Some(region.to_string()),
            Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::Kubernetes(id.to_string(), name.to_string()),
        );

        let template_directory = format!("{}/aws/bootstrap", context.lib_root_dir());

        let mut aws_zones: Vec<AwsZones> = Vec::with_capacity(3);
        for zone in zones {
            match AwsZones::from_string(zone.to_string()) {
                Ok(x) => aws_zones.push(x),
                Err(e) => {
                    return Err(EngineError::new_unsupported_zone(
                        event_details.clone(),
                        region.to_string(),
                        zone.to_string(),
                        CommandError::new_from_safe_message(e.to_string()),
                    ))
                }
            };
        }

        for node_group in &nodes_groups {
            if let Err(e) = AwsInstancesType::from_str(node_group.instance_type.as_str()) {
                let err = EngineError::new_unsupported_instance_type(
                    event_details.clone(),
                    node_group.instance_type.as_str(),
                    e,
                );

                logger.log(LogLevel::Error, EngineEvent::Error(err.clone()));

                return Err(err);
            }
        }

        // TODO export this
        let s3 = S3::new(
            context.clone(),
            "s3-temp-id".to_string(),
            "default-s3".to_string(),
            cloud_provider.access_key_id().clone(),
            cloud_provider.secret_access_key().clone(),
            region.clone(),
            true,
            context.resource_expiration_in_seconds(),
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

    /// divide by 2 the total number of subnet to get the exact same number as private and public
    fn check_odd_subnets(
        &self,
        event_details: EventDetails,
        zone_name: &str,
        subnet_block: &Vec<String>,
    ) -> Result<usize, EngineError> {
        if subnet_block.len() % 2 == 1 {
            return Err(EngineError::new_subnets_count_is_not_even(
                event_details,
                zone_name.to_string(),
                subnet_block.len(),
            ));
        }

        Ok((subnet_block.len() / 2) as usize)
    }

    fn set_cluster_autoscaler_replicas(
        &self,
        event_details: EventDetails,
        replicas_count: u32,
    ) -> Result<(), EngineError> {
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe(format!("Scaling cluster autoscaler to `{}`.", replicas_count)),
            ),
        );
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

    fn tera_context(&self) -> Result<TeraContext, EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::LoadConfiguration));
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
                let max_subnet_zone_a =
                    self.check_odd_subnets(event_details.clone(), "a", &eks_zone_a_subnet_blocks_private)?;
                let max_subnet_zone_b =
                    self.check_odd_subnets(event_details.clone(), "b", &eks_zone_b_subnet_blocks_private)?;
                let max_subnet_zone_c =
                    self.check_odd_subnets(event_details.clone(), "c", &eks_zone_c_subnet_blocks_private)?;

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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
        let listeners_helper = ListenersHelper::new(&self.listeners);

        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Preparing EKS cluster deployment.".to_string()),
            ),
        );
        self.send_to_customer(
            format!("Preparing EKS {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
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

        // create AWS IAM roles
        let already_created_roles = get_default_roles_to_create();
        for role in already_created_roles {
            match role.create_service_linked_role(
                self.cloud_provider.access_key_id().as_str(),
                self.cloud_provider.secret_access_key().as_str(),
            ) {
                Ok(_) => self.logger().log(
                    LogLevel::Info,
                    EngineEvent::Deploying(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!(
                            "Role {} is already present, no need to create",
                            role.role_name
                        )),
                    ),
                ),
                Err(e) => self.logger().log(
                    LogLevel::Error,
                    EngineEvent::Error(EngineError::new_cannot_get_or_create_iam_role(
                        event_details.clone(),
                        role.role_name,
                        e,
                    )),
                ),
            }
        }

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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
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
                EventMessage::new_from_safe("Deploying EKS cluster.".to_string()),
            ),
        );
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

        // terraform deployment dedicated to cloud resources
        if let Err(e) = terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            return Err(EngineError::new_terraform_error_while_executing_pipeline(
                event_details.clone(),
                e,
            ));
        }

        // kubernetes helm deployments on the cluster
        // todo: instead of downloading kubeconfig file, use the one that has just been generated by terraform
        let kubeconfig_path = &self.get_kubeconfig_file_path()?;
        let kubeconfig_path = Path::new(kubeconfig_path);

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

        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
            ),
        );
        let helm_charts_to_deploy = aws_helm_charts(
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
                EventMessage::new_from_safe("EKS.create_error() called.".to_string()),
            ),
        );

        match kubectl_exec_get_events(kubeconfig_path, None, environment_variables) {
            Ok(ok_line) => self.logger().log(
                LogLevel::Info,
                EngineEvent::Deploying(event_details.clone(), EventMessage::new(ok_line, None)),
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
                EventMessage::new_from_safe("EKS.upgrade_error() called.".to_string()),
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
                EventMessage::new_from_safe("EKS.downgrade_error() called.".to_string()),
            ),
        );

        Ok(())
    }

    fn pause(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));
        let listeners_helper = ListenersHelper::new(&self.listeners);

        self.send_to_customer(
            format!("Preparing EKS {} cluster pause with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );

        self.logger().log(
            LogLevel::Info,
            EngineEvent::Pausing(
                self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
                EventMessage::new_from_safe("Preparing EKS cluster pause.".to_string()),
            ),
        );

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
        let worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
        context.insert("eks_worker_nodes", &worker_nodes);

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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
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
                    if name.starts_with("aws_eks_node_group.") {
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
            return Err(EngineError::new_cluster_has_no_worker_nodes(
                event_details.clone(),
                None,
            ));
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
                                OperationResult::Retry(
                                    EngineError::new_cannot_get_k8s_api_custom_metrics(event_details.clone(), e))
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
            format!("Pausing EKS {} cluster deployment with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Pausing(
                event_details.clone(),
                EventMessage::new_from_safe("Pausing EKS cluster deployment.".to_string()),
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
                EventMessage::new_from_safe("EKS.pause_error() called.".to_string()),
            ),
        );

        Ok(())
    }

    fn delete(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let mut skip_kubernetes_step = false;

        self.send_to_customer(
            format!("Preparing to delete EKS cluster {} with id {}", self.name(), self.id()).as_str(),
            &listeners_helper,
        );
        self.logger().log(
            LogLevel::Warning,
            EngineEvent::Deleting(
                event_details.clone(),
                EventMessage::new_from_safe("Preparing to delete EKS cluster.".to_string()),
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

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
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
                EventMessage::new_from_safe("EKS.delete_error() called.".to_string()),
            ),
        );

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
                "Start preparing EKS upgrade process {} cluster with id {}",
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
                EventMessage::new_from_safe("Start preparing EKS cluster upgrade process".to_string()),
            ),
        );

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context()?;

        //
        // Upgrade master nodes
        //
        match &kubernetes_upgrade_status.required_upgrade_on {
            Some(KubernetesNodesType::Masters) => {
                self.send_to_customer(
                    format!(
                        "Start upgrading process for master nodes on {}/{}",
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
                        EventMessage::new_from_safe("Start upgrading process for master nodes.".to_string()),
                    ),
                );

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
                        event_details.clone(),
                        self.template_directory.to_string(),
                        temp_dir.to_string(),
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
                        event_details.clone(),
                        common_bootstrap_charts.to_string(),
                        common_charts_temp_dir.to_string(),
                        e,
                    ));
                }

                self.send_to_customer(
                    format!("Upgrading Kubernetes {} master nodes", self.name()).as_str(),
                    &listeners_helper,
                );
                self.logger().log(
                    LogLevel::Info,
                    EngineEvent::Deploying(
                        event_details.clone(),
                        EventMessage::new_from_safe("Upgrading Kubernetes master nodes.".to_string()),
                    ),
                );

                match terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
                    Ok(_) => {
                        self.send_to_customer(
                            format!(
                                "Kubernetes {} master nodes have been successfully upgraded",
                                self.name()
                            )
                            .as_str(),
                            &listeners_helper,
                        );
                        self.logger().log(
                            LogLevel::Info,
                            EngineEvent::Deploying(
                                event_details.clone(),
                                EventMessage::new_from_safe(
                                    "Kubernetes master nodes have been successfully upgraded.".to_string(),
                                ),
                            ),
                        );
                    }
                    Err(e) => {
                        return Err(EngineError::new_terraform_error_while_executing_pipeline(
                            event_details.clone(),
                            e,
                        ));
                    }
                }
            }
            Some(KubernetesNodesType::Workers) => {
                self.logger().log(
                    LogLevel::Info,
                    EngineEvent::Deploying(
                        event_details.clone(),
                        EventMessage::new_from_safe(
                            "No need to perform Kubernetes master upgrade, they are already up to date.".to_string(),
                        ),
                    ),
                );
            }
            None => {
                self.logger().log(
                    LogLevel::Info,
                    EngineEvent::Deploying(
                        event_details.clone(),
                        EventMessage::new_from_safe(
                            "No Kubernetes upgrade required, masters and workers are already up to date.".to_string(),
                        ),
                    ),
                );
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
            self.logger().log(LogLevel::Error, EngineEvent::Error(e.clone()));
            return Err(e);
        }

        //
        // Upgrade worker nodes
        //
        self.send_to_customer(
            format!(
                "Preparing workers nodes for upgrade for Kubernetes cluster {}",
                self.name()
            )
            .as_str(),
            &listeners_helper,
        );
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Preparing workers nodes for upgrade for Kubernetes cluster.".to_string()),
            ),
        );

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
                event_details.clone(),
                self.template_directory.to_string(),
                temp_dir.to_string(),
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
                event_details.clone(),
                common_bootstrap_charts.to_string(),
                common_charts_temp_dir.to_string(),
                e,
            ));
        }

        self.send_to_customer(
            format!("Upgrading Kubernetes {} worker nodes", self.name()).as_str(),
            &listeners_helper,
        );
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Deploying(
                event_details.clone(),
                EventMessage::new_from_safe("Upgrading Kubernetes worker nodes.".to_string()),
            ),
        );

        // Disable cluster autoscaler deployment
        let _ = self.set_cluster_autoscaler_replicas(event_details.clone(), 0)?;

        match terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            Ok(_) => {
                self.send_to_customer(
                    format!(
                        "Kubernetes {} workers nodes have been successfully upgraded",
                        self.name()
                    )
                    .as_str(),
                    &listeners_helper,
                );
                self.logger().log(
                    LogLevel::Info,
                    EngineEvent::Deploying(
                        event_details.clone(),
                        EventMessage::new_from_safe(
                            "Kubernetes workers nodes have been successfully upgraded.".to_string(),
                        ),
                    ),
                );
            }
            Err(e) => {
                // enable cluster autoscaler deployment
                let _ = self.set_cluster_autoscaler_replicas(event_details.clone(), 1)?;

                return Err(EngineError::new_terraform_error_while_executing_pipeline(
                    event_details.clone(),
                    e,
                ));
            }
        }

        // enable cluster autoscaler deployment
        self.set_cluster_autoscaler_replicas(event_details.clone(), 1)
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

impl<'a> Listen for EKS<'a> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
