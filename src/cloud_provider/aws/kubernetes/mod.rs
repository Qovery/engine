use core::fmt;
use std::env;
use std::path::Path;

use retry::delay::{Fibonacci, Fixed};
use retry::Error::Operation;
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::aws::kubernetes::helm_charts::{aws_helm_charts, ChartsConfigPrerequisites};
use crate::cloud_provider::aws::kubernetes::roles::get_default_roles_to_create;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use crate::cloud_provider::helm::{deploy_charts_levels, ChartInfo};
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, uninstall_cert_manager, Kubernetes, ProviderOptions,
};
use crate::cloud_provider::models::{NodeGroups, NodeGroupsFormat};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::CloudProvider;
use crate::cmd;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::cmd::kubectl::{kubectl_exec_api_custom_metrics, kubectl_exec_get_all_namespaces, kubectl_exec_get_events};
use crate::cmd::terraform::{terraform_exec, terraform_init_validate_plan_apply, terraform_init_validate_state_list};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider;
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
use crate::io_models::{Context, Features, ListenersHelper, QoveryIdentifier, ToHelmString, ToTerraformString};
use crate::object_storage::s3::S3;
use crate::string::terraform_list_format;

pub mod ec2;
pub mod eks;
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
    pub ec2_zone_a_subnet_blocks: Vec<String>,
    pub ec2_zone_b_subnet_blocks: Vec<String>,
    pub ec2_zone_c_subnet_blocks: Vec<String>,
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
    pub ec2_cidr_subnet: String,
    pub vpc_custom_routing_table: Vec<VpcCustomRoutingTable>,
    pub eks_access_cidr_blocks: Vec<String>,
    pub ec2_access_cidr_blocks: Vec<String>,
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

fn event_details<S: Into<String>>(
    cloud_provider: &Box<dyn CloudProvider>,
    kubernetes_id: S,
    kubernetes_name: S,
    kubernetes_region: &AwsRegion,
    context: &Context,
) -> EventDetails {
    EventDetails::new(
        Some(cloud_provider.kind()),
        QoveryIdentifier::new_from_long_id(context.organization_id().to_string()),
        QoveryIdentifier::new_from_long_id(context.cluster_id().to_string()),
        QoveryIdentifier::new_from_long_id(context.execution_id().to_string()),
        Some(kubernetes_region.to_string()),
        Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
        Transmitter::Kubernetes(kubernetes_id.into(), kubernetes_name.into()),
    )
}

fn aws_zones(
    zones: Vec<String>,
    region: &AwsRegion,
    event_details: &EventDetails,
) -> Result<Vec<AwsZones>, EngineError> {
    let mut aws_zones = vec![];

    for zone in zones {
        match AwsZones::from_string(zone.to_string()) {
            Ok(x) => aws_zones.push(x),
            Err(e) => {
                return Err(EngineError::new_unsupported_zone(
                    event_details.clone(),
                    region.to_string(),
                    zone,
                    CommandError::new_from_safe_message(e.to_string()),
                ));
            }
        };
    }

    Ok(aws_zones)
}

fn s3(context: &Context, region: &AwsRegion, cloud_provider: &dyn CloudProvider) -> S3 {
    S3::new(
        context.clone(),
        "s3-temp-id".to_string(),
        "default-s3".to_string(),
        cloud_provider.access_key_id(),
        cloud_provider.secret_access_key(),
        region.clone(),
        true,
        context.resource_expiration_in_seconds(),
    )
}

/// divide by 2 the total number of subnet to get the exact same number as private and public
fn check_odd_subnets(
    event_details: EventDetails,
    zone_name: &str,
    subnet_block: &[String],
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

fn lets_encrypt_url(context: &Context) -> String {
    match context.is_test_cluster() {
        true => "https://acme-staging-v02.api.letsencrypt.org/directory",
        false => "https://acme-v02.api.letsencrypt.org/directory",
    }
    .to_string()
}

fn managed_dns_resolvers_terraform_format(dns_provider: &dyn DnsProvider) -> String {
    let managed_dns_resolvers = dns_provider
        .resolvers()
        .iter()
        .map(|x| format!("{}", x.clone()))
        .collect::<Vec<_>>();

    terraform_list_format(managed_dns_resolvers)
}

fn tera_context(
    kubernetes: &dyn Kubernetes,
    zones: &Vec<AwsZones>,
    node_groups: &[NodeGroups],
    options: &Options,
) -> Result<TeraContext, EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::LoadConfiguration));
    let mut context = TeraContext::new();

    let format_ips =
        |ips: &Vec<String>| -> Vec<String> { ips.iter().map(|ip| format!("\"{}\"", ip)).collect::<Vec<_>>() };

    let aws_zones = zones
        .iter()
        .map(|zone| zone.to_terraform_format_string())
        .collect::<Vec<_>>();

    let mut eks_zone_a_subnet_blocks_private = format_ips(&options.eks_zone_a_subnet_blocks);
    let mut eks_zone_b_subnet_blocks_private = format_ips(&options.eks_zone_b_subnet_blocks);
    let mut eks_zone_c_subnet_blocks_private = format_ips(&options.eks_zone_c_subnet_blocks);

    match options.vpc_qovery_network_mode {
        VpcQoveryNetworkMode::WithNatGateways => {
            let max_subnet_zone_a = check_odd_subnets(event_details.clone(), "a", &eks_zone_a_subnet_blocks_private)?;
            let max_subnet_zone_b = check_odd_subnets(event_details.clone(), "b", &eks_zone_b_subnet_blocks_private)?;
            let max_subnet_zone_c = check_odd_subnets(event_details.clone(), "c", &eks_zone_c_subnet_blocks_private)?;

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

    let mut ec2_zone_a_subnet_blocks_private = format_ips(&options.ec2_zone_a_subnet_blocks);
    let mut ec2_zone_b_subnet_blocks_private = format_ips(&options.ec2_zone_b_subnet_blocks);
    let mut ec2_zone_c_subnet_blocks_private = format_ips(&options.ec2_zone_c_subnet_blocks);

    match options.vpc_qovery_network_mode {
        VpcQoveryNetworkMode::WithNatGateways => {
            let max_subnet_zone_a = check_odd_subnets(event_details.clone(), "a", &ec2_zone_a_subnet_blocks_private)?;
            let max_subnet_zone_b = check_odd_subnets(event_details.clone(), "b", &ec2_zone_b_subnet_blocks_private)?;
            let max_subnet_zone_c = check_odd_subnets(event_details.clone(), "c", &ec2_zone_c_subnet_blocks_private)?;

            let ec2_zone_a_subnet_blocks_public: Vec<String> =
                ec2_zone_a_subnet_blocks_private.drain(max_subnet_zone_a..).collect();
            let ec2_zone_b_subnet_blocks_public: Vec<String> =
                ec2_zone_b_subnet_blocks_private.drain(max_subnet_zone_b..).collect();
            let ec2_zone_c_subnet_blocks_public: Vec<String> =
                ec2_zone_c_subnet_blocks_private.drain(max_subnet_zone_c..).collect();

            context.insert("ec2_zone_a_subnet_blocks_public", &ec2_zone_a_subnet_blocks_public);
            context.insert("ec2_zone_b_subnet_blocks_public", &ec2_zone_b_subnet_blocks_public);
            context.insert("ec2_zone_c_subnet_blocks_public", &ec2_zone_c_subnet_blocks_public);
        }
        VpcQoveryNetworkMode::WithoutNatGateways => {}
    };

    context.insert("vpc_qovery_network_mode", &options.vpc_qovery_network_mode.to_string());

    let rds_zone_a_subnet_blocks = format_ips(&options.rds_zone_a_subnet_blocks);
    let rds_zone_b_subnet_blocks = format_ips(&options.rds_zone_b_subnet_blocks);
    let rds_zone_c_subnet_blocks = format_ips(&options.rds_zone_c_subnet_blocks);

    let documentdb_zone_a_subnet_blocks = format_ips(&options.documentdb_zone_a_subnet_blocks);
    let documentdb_zone_b_subnet_blocks = format_ips(&options.documentdb_zone_b_subnet_blocks);
    let documentdb_zone_c_subnet_blocks = format_ips(&options.documentdb_zone_c_subnet_blocks);

    let elasticache_zone_a_subnet_blocks = format_ips(&options.elasticache_zone_a_subnet_blocks);
    let elasticache_zone_b_subnet_blocks = format_ips(&options.elasticache_zone_b_subnet_blocks);
    let elasticache_zone_c_subnet_blocks = format_ips(&options.elasticache_zone_c_subnet_blocks);

    let elasticsearch_zone_a_subnet_blocks = format_ips(&options.elasticsearch_zone_a_subnet_blocks);
    let elasticsearch_zone_b_subnet_blocks = format_ips(&options.elasticsearch_zone_b_subnet_blocks);
    let elasticsearch_zone_c_subnet_blocks = format_ips(&options.elasticsearch_zone_c_subnet_blocks);

    let region_cluster_id = format!("{}-{}", kubernetes.region(), kubernetes.id());
    let vpc_cidr_block = options.vpc_cidr_block.clone();
    let eks_cloudwatch_log_group = format!("/aws/eks/{}/cluster", kubernetes.id());
    let eks_cidr_subnet = options.eks_cidr_subnet.clone();
    let ec2_cidr_subnet = options.ec2_cidr_subnet.clone();

    let eks_access_cidr_blocks = format_ips(&options.eks_access_cidr_blocks);
    let ec2_access_cidr_blocks = format_ips(&options.ec2_access_cidr_blocks);

    let qovery_api_url = options.qovery_api_url.clone();
    let rds_cidr_subnet = options.rds_cidr_subnet.clone();
    let documentdb_cidr_subnet = options.documentdb_cidr_subnet.clone();
    let elasticache_cidr_subnet = options.elasticache_cidr_subnet.clone();
    let elasticsearch_cidr_subnet = options.elasticsearch_cidr_subnet.clone();

    // Qovery
    context.insert("organization_id", kubernetes.cloud_provider().organization_id());
    context.insert("qovery_api_url", &qovery_api_url);

    context.insert("engine_version_controller_token", &options.engine_version_controller_token);
    context.insert("agent_version_controller_token", &options.agent_version_controller_token);

    context.insert("test_cluster", &kubernetes.context().is_test_cluster());

    if let Some(resource_expiration_in_seconds) = kubernetes.context().resource_expiration_in_seconds() {
        context.insert("resource_expiration_in_seconds", &resource_expiration_in_seconds);
    }

    context.insert("force_upgrade", &kubernetes.context().requires_forced_upgrade());

    // Qovery features
    context.insert(
        "log_history_enabled",
        &kubernetes.context().is_feature_enabled(&Features::LogsHistory),
    );
    context.insert(
        "metrics_history_enabled",
        &kubernetes.context().is_feature_enabled(&Features::MetricsHistory),
    );

    // DNS configuration
    let managed_dns_list = vec![kubernetes.dns_provider().name()];
    let managed_dns_domains_helm_format = vec![kubernetes.dns_provider().domain().to_string()];
    let managed_dns_domains_root_helm_format = vec![kubernetes.dns_provider().domain().root_domain().to_string()];
    let managed_dns_domains_terraform_format =
        terraform_list_format(vec![kubernetes.dns_provider().domain().to_string()]);
    let managed_dns_domains_root_terraform_format =
        terraform_list_format(vec![kubernetes.dns_provider().domain().root_domain().to_string()]);
    let managed_dns_resolvers_terraform_format = managed_dns_resolvers_terraform_format(kubernetes.dns_provider());

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

    match kubernetes.dns_provider().kind() {
        dns_provider::Kind::Cloudflare => {
            context.insert("external_dns_provider", kubernetes.dns_provider().provider_name());
            context.insert("cloudflare_api_token", kubernetes.dns_provider().token());
            context.insert("cloudflare_email", kubernetes.dns_provider().account());
        }
    };

    context.insert("dns_email_report", &options.tls_email_report);

    // TLS
    context.insert("acme_server_url", &lets_encrypt_url(kubernetes.context()));

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
                    None => kubernetes.logger().log(EngineEvent::Error(
                        EngineError::new_missing_required_env_variable(event_details, "VAULT_SECRET_ID".to_string()),
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

    // Other Kubernetes
    context.insert("kubernetes_cluster_name", &kubernetes.cluster_name());
    context.insert("enable_cluster_autoscaler", &true);

    // AWS
    context.insert("aws_access_key", &kubernetes.cloud_provider().access_key_id());
    context.insert("aws_secret_key", &kubernetes.cloud_provider().secret_access_key());

    // AWS S3 tfstate storage
    context.insert(
        "aws_access_key_tfstates_account",
        kubernetes
            .cloud_provider()
            .terraform_state_credentials()
            .access_key_id
            .as_str(),
    );

    context.insert(
        "aws_secret_key_tfstates_account",
        kubernetes
            .cloud_provider()
            .terraform_state_credentials()
            .secret_access_key
            .as_str(),
    );
    context.insert(
        "aws_region_tfstates_account",
        kubernetes
            .cloud_provider()
            .terraform_state_credentials()
            .region
            .as_str(),
    );

    context.insert("aws_region", &kubernetes.region());
    context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");
    context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");
    context.insert("vpc_cidr_block", &vpc_cidr_block);
    context.insert("vpc_custom_routing_table", &options.vpc_custom_routing_table);
    context.insert("s3_kubeconfig_bucket", &format!("qovery-kubeconfigs-{}", kubernetes.id()));

    // AWS - EKS
    context.insert("aws_availability_zones", &aws_zones);
    context.insert("eks_cidr_subnet", &eks_cidr_subnet);
    context.insert("ec2_cidr_subnet", &ec2_cidr_subnet);
    context.insert("kubernetes_cluster_name", kubernetes.name());
    context.insert("kubernetes_cluster_id", kubernetes.id());
    context.insert("kubernetes_full_cluster_id", kubernetes.context().cluster_id());
    context.insert("eks_region_cluster_id", region_cluster_id.as_str());
    context.insert("eks_worker_nodes", &node_groups);
    context.insert("ec2_zone_a_subnet_blocks_private", &ec2_zone_a_subnet_blocks_private);
    context.insert("ec2_zone_b_subnet_blocks_private", &ec2_zone_b_subnet_blocks_private);
    context.insert("ec2_zone_c_subnet_blocks_private", &ec2_zone_c_subnet_blocks_private);
    context.insert("eks_zone_a_subnet_blocks_private", &eks_zone_a_subnet_blocks_private);
    context.insert("eks_zone_b_subnet_blocks_private", &eks_zone_b_subnet_blocks_private);
    context.insert("eks_zone_c_subnet_blocks_private", &eks_zone_c_subnet_blocks_private);
    context.insert("eks_masters_version", &kubernetes.version());
    context.insert("eks_workers_version", &kubernetes.version());
    context.insert("ec2_masters_version", &kubernetes.version());
    context.insert("ec2_workers_version", &kubernetes.version());
    context.insert("eks_cloudwatch_log_group", &eks_cloudwatch_log_group);
    context.insert("eks_access_cidr_blocks", &eks_access_cidr_blocks);
    context.insert("ec2_access_cidr_blocks", &ec2_access_cidr_blocks);

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
    context.insert("elasticsearch_zone_a_subnet_blocks", &elasticsearch_zone_a_subnet_blocks);
    context.insert("elasticsearch_zone_b_subnet_blocks", &elasticsearch_zone_b_subnet_blocks);
    context.insert("elasticsearch_zone_c_subnet_blocks", &elasticsearch_zone_c_subnet_blocks);

    // grafana credentials
    context.insert("grafana_admin_user", options.grafana_admin_user.as_str());
    context.insert("grafana_admin_password", options.grafana_admin_password.as_str());

    // qovery
    context.insert("qovery_api_url", options.qovery_api_url.as_str());
    context.insert("qovery_nats_url", options.qovery_nats_url.as_str());
    context.insert("qovery_nats_user", options.qovery_nats_user.as_str());
    context.insert("qovery_nats_password", options.qovery_nats_password.as_str());
    context.insert("qovery_ssh_key", options.qovery_ssh_key.as_str());
    context.insert("discord_api_key", options.discord_api_key.as_str());

    Ok(context)
}

fn create(
    kubernetes: &dyn Kubernetes,
    kubernetes_long_id: uuid::Uuid,
    template_directory: &str,
    aws_zones: &Vec<AwsZones>,
    node_groups: &Vec<NodeGroups>,
    options: &Options,
) -> Result<(), EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing EKS cluster deployment.".to_string()),
    ));

    kubernetes.send_to_customer(
        format!(
            "Preparing {} {} cluster deployment with id {}",
            kubernetes.kind(),
            kubernetes.name(),
            kubernetes.id()
        )
        .as_str(),
        &listeners_helper,
    );

    // upgrade cluster instead if required
    match kubernetes.get_kubeconfig_file() {
        Ok((path, _)) => match is_kubernetes_upgrade_required(
            path,
            kubernetes.version(),
            kubernetes.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            kubernetes.logger(),
        ) {
            Ok(x) => {
                if x.required_upgrade_on.is_some() {
                    return kubernetes.upgrade_with_status(x);
                }

                kubernetes.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                ))
            }
            Err(e) => {
                kubernetes.logger().log(EngineEvent::Error(e, Some(EventMessage::new_from_safe(
                    "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                ))));
            }
        },
        Err(_) => kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Kubernetes cluster upgrade not required, config file is not found and cluster have certainly never been deployed before".to_string())))
    };

    // create AWS IAM roles
    let already_created_roles = get_default_roles_to_create();
    for role in already_created_roles {
        match role.create_service_linked_role(
            kubernetes.cloud_provider().access_key_id().as_str(),
            kubernetes.cloud_provider().secret_access_key().as_str(),
        ) {
            Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!("Role {} is already present, no need to create", role.role_name)),
            )),
            Err(e) => kubernetes.logger().log(EngineEvent::Error(
                EngineError::new_cannot_get_or_create_iam_role(event_details.clone(), role.role_name, e),
                None,
            )),
        }
    }

    let temp_dir = kubernetes.get_temp_dir(event_details.clone())?;

    // generate terraform files and copy them into temp dir
    let context = tera_context(kubernetes, aws_zones, node_groups, options)?;

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
    {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir,
            e,
        ));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        ));
    }

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Deploying {} cluster.", kubernetes.kind())),
    ));

    kubernetes.send_to_customer(
        format!(
            "Deploying {} {} cluster deployment with id {}",
            kubernetes.kind(),
            kubernetes.name(),
            kubernetes.id()
        )
        .as_str(),
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
                            Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                                event_details.clone(),
                                EventMessage::new_from_safe(format!("successfully removed {}", &entry)),
                            )),
                            Err(e) => {
                                return Err(EngineError::new_terraform_cannot_remove_entry_out(
                                    event_details,
                                    entry.to_string(),
                                    e,
                                ));
                            }
                        }
                    };
                }
            }
        }
        Err(e) => kubernetes.logger().log(EngineEvent::Error(
            EngineError::new_terraform_state_does_not_exist(event_details.clone(), e),
            None,
        )),
    };

    // terraform deployment dedicated to cloud resources
    if let Err(e) = terraform_init_validate_plan_apply(temp_dir.as_str(), kubernetes.context().is_dry_run_deploy()) {
        return Err(EngineError::new_terraform_error_while_executing_pipeline(event_details, e));
    }

    // kubernetes helm deployments on the cluster
    // todo: instead of downloading kubeconfig file, use the one that has just been generated by terraform
    let kubeconfig_path = kubernetes.get_kubeconfig_file_path()?;
    let kubeconfig_path = Path::new(&kubeconfig_path);

    let credentials_environment_variables: Vec<(String, String)> = kubernetes
        .cloud_provider()
        .credentials_environment_variables()
        .into_iter()
        .map(|x| (x.0.to_string(), x.1.to_string()))
        .collect();

    let charts_prerequisites = ChartsConfigPrerequisites {
        organization_id: kubernetes.cloud_provider().organization_id().to_string(),
        organization_long_id: kubernetes.cloud_provider().organization_long_id(),
        infra_options: options.clone(),
        cluster_id: kubernetes.id().to_string(),
        cluster_long_id: kubernetes_long_id,
        region: kubernetes.region(),
        cluster_name: kubernetes.cluster_name(),
        cloud_provider: "aws".to_string(),
        test_cluster: kubernetes.context().is_test_cluster(),
        aws_access_key_id: kubernetes.cloud_provider().access_key_id(),
        aws_secret_access_key: kubernetes.cloud_provider().secret_access_key(),
        vpc_qovery_network_mode: options.vpc_qovery_network_mode.clone(),
        qovery_engine_location: options.qovery_engine_location.clone(),
        ff_log_history_enabled: kubernetes.context().is_feature_enabled(&Features::LogsHistory),
        ff_metrics_history_enabled: kubernetes.context().is_feature_enabled(&Features::MetricsHistory),
        managed_dns_name: kubernetes.dns_provider().domain().to_string(),
        managed_dns_helm_format: kubernetes.dns_provider().domain().to_helm_format_string(),
        managed_dns_resolvers_terraform_format: managed_dns_resolvers_terraform_format(kubernetes.dns_provider()),
        external_dns_provider: kubernetes.dns_provider().provider_name().to_string(),
        dns_email_report: options.tls_email_report.clone(),
        acme_url: lets_encrypt_url(kubernetes.context()),
        cloudflare_email: kubernetes.dns_provider().account().to_string(),
        cloudflare_api_token: kubernetes.dns_provider().token().to_string(),
        disable_pleco: kubernetes.context().disable_pleco(),
    };

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
    ));

    let helm_charts_to_deploy = aws_helm_charts(
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
        kubernetes.context().is_dry_run_deploy(),
    )
    .map_err(|e| EngineError::new_helm_charts_deploy_error(event_details.clone(), e))
}

fn create_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
    let (kubeconfig_path, _) = kubernetes.get_kubeconfig_file()?;
    let environment_variables = kubernetes.cloud_provider().credentials_environment_variables();

    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create)),
        EventMessage::new_from_safe(format!("{}.create_error() called.", kubernetes.kind())),
    ));

    match kubectl_exec_get_events(kubeconfig_path, None, environment_variables) {
        Ok(ok_line) => kubernetes
            .logger()
            .log(EngineEvent::Info(event_details, EventMessage::new(ok_line, None))),
        Err(err) => kubernetes.logger().log(EngineEvent::Warning(
            event_details,
            EventMessage::new(
                "Error trying to get kubernetes events".to_string(),
                Some(err.message(ErrorMessageVerbosity::FullDetails)),
            ),
        )),
    };

    Ok(())
}

fn upgrade_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade)),
        EventMessage::new_from_safe(format!("{}.upgrade_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

fn downgrade() -> Result<(), EngineError> {
    Ok(())
}

fn downgrade_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Downgrade)),
        EventMessage::new_from_safe(format!("{}.downgrade_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

fn pause(
    kubernetes: &dyn Kubernetes,
    template_directory: &str,
    aws_zones: &Vec<AwsZones>,
    node_groups: &Vec<NodeGroups>,
    options: &Options,
) -> Result<(), EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());

    kubernetes.send_to_customer(
        format!(
            "Preparing {} {} cluster pause with id {}",
            kubernetes.kind(),
            kubernetes.name(),
            kubernetes.id()
        )
        .as_str(),
        &listeners_helper,
    );

    kubernetes.logger().log(EngineEvent::Info(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
        EventMessage::new_from_safe(format!("Preparing {} cluster pause.", kubernetes.kind())),
    ));

    let temp_dir = kubernetes.get_temp_dir(event_details.clone())?;

    // generate terraform files and copy them into temp dir
    let mut context = tera_context(kubernetes, aws_zones, node_groups, options)?;

    // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
    let worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
    context.insert("eks_worker_nodes", &worker_nodes);

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
    {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir,
            e,
        ));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap-{type}/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap-{type}/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
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
                if name.starts_with("aws_eks_node_group.") {
                    tf_workers_resources_name.push(name);
                }
            }
            tf_workers_resources_name
        }
        Err(e) => {
            let error = EngineError::new_terraform_state_does_not_exist(event_details, e);
            kubernetes.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(error);
        }
    };

    if tf_workers_resources.is_empty() {
        return Err(EngineError::new_cluster_has_no_worker_nodes(event_details, None));
    }

    let kubernetes_config_file_path = kubernetes.get_kubeconfig_file_path()?;

    // pause: wait 1h for the engine to have 0 running jobs before pausing and avoid getting unreleased lock (from helm or terraform for example)
    if options.qovery_engine_location == EngineLocation::ClientSide {
        match kubernetes.context().is_feature_enabled(&Features::MetricsHistory) {
            true => {
                let metric_name = "taskmanager_nb_running_tasks";
                let wait_engine_job_finish = retry::retry(Fixed::from_millis(60000).take(60), || {
                    return match kubectl_exec_api_custom_metrics(
                        &kubernetes_config_file_path,
                        kubernetes.cloud_provider().credentials_environment_variables(),
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
                                        return OperationResult::Retry(EngineError::new_cannot_get_k8s_api_custom_metrics(
                                            event_details.clone(),
                                            CommandError::new("Error while looking at the API metric value".to_string(), Some(e.to_string()), None)));
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
                        kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("No current running jobs on the Engine, infrastructure pause is allowed to start".to_string())));
                    }
                    Err(Operation { error, .. }) => {
                        return Err(error);
                    }
                    Err(retry::Error::Internal(msg)) => {
                        return Err(EngineError::new_cannot_pause_cluster_tasks_are_running(event_details, Some(CommandError::new_from_safe_message(msg))));
                    }
                }
            }
            false => kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Engines are running Client side, but metric history flag is disabled. You will encounter issues during cluster lifecycles if you do not enable metric history".to_string()))),
        }
    }

    let mut terraform_args_string = vec!["apply".to_string(), "-auto-approve".to_string()];
    for x in tf_workers_resources {
        terraform_args_string.push(format!("-target={}", x));
    }
    let terraform_args = terraform_args_string.iter().map(|x| &**x).collect();

    kubernetes.send_to_customer(
        format!(
            "Pausing {} {} cluster deployment with id {}",
            kubernetes.kind(),
            kubernetes.name(),
            kubernetes.id()
        )
        .as_str(),
        &listeners_helper,
    );

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Pausing EKS cluster deployment.".to_string()),
    ));

    match terraform_exec(temp_dir.as_str(), terraform_args) {
        Ok(_) => {
            let message = format!("Kubernetes cluster {} successfully paused", kubernetes.name());
            kubernetes.send_to_customer(&message, &listeners_helper);
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details, EventMessage::new_from_safe(message)));

            Ok(())
        }
        Err(e) => Err(EngineError::new_terraform_error_while_executing_pipeline(event_details, e)),
    }
}

fn pause_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
        EventMessage::new_from_safe(format!("{}.pause_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

fn delete(
    kubernetes: &dyn Kubernetes,
    template_directory: &str,
    aws_zones: &Vec<AwsZones>,
    node_groups: &Vec<NodeGroups>,
    options: &Options,
) -> Result<(), EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
    let listeners_helper = ListenersHelper::new(kubernetes.listeners());
    let mut skip_kubernetes_step = false;

    kubernetes.send_to_customer(
        format!(
            "Preparing to delete {} cluster {} with id {}",
            kubernetes.kind(),
            kubernetes.name(),
            kubernetes.id()
        )
        .as_str(),
        &listeners_helper,
    );
    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Preparing to delete {} cluster.", kubernetes.kind())),
    ));

    let temp_dir = kubernetes.get_temp_dir(event_details.clone())?;

    // generate terraform files and copy them into temp dir
    let context = tera_context(kubernetes, aws_zones, node_groups, options)?;

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
    {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir,
            e,
        ));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        ));
    }

    let kubernetes_config_file_path = match kubernetes.get_kubeconfig_file_path() {
        Ok(x) => x,
        Err(e) => {
            let safe_message = "Skipping Kubernetes uninstall because it can't be reached.";
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(safe_message.to_string(), Some(e.message(ErrorMessageVerbosity::FullDetails))),
            ));

            skip_kubernetes_step = true;
            "".to_string()
        }
    };

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    let message = format!(
        "Ensuring everything is up to date before deleting cluster {}/{}",
        kubernetes.name(),
        kubernetes.id()
    );

    kubernetes.send_to_customer(&message, &listeners_helper);
    kubernetes
        .logger()
        .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform apply before running a delete.".to_string()),
    ));

    if let Err(e) = cmd::terraform::terraform_init_validate_plan_apply(temp_dir.as_str(), false) {
        // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
        kubernetes.logger().log(EngineEvent::Error(
            EngineError::new_terraform_error_while_executing_pipeline(event_details.clone(), e),
            None,
        ));
    };

    if !skip_kubernetes_step {
        // should make the diff between all namespaces and qovery managed namespaces
        let message = format!(
            "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
            kubernetes.name(),
            kubernetes.id()
        );

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(message.to_string()),
        ));

        kubernetes.send_to_customer(&message, &listeners_helper);

        let all_namespaces = kubectl_exec_get_all_namespaces(
            &kubernetes_config_file_path,
            kubernetes.cloud_provider().credentials_environment_variables(),
        );

        match all_namespaces {
            Ok(namespace_vec) => {
                let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                kubernetes.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Deleting non Qovery namespaces".to_string()),
                ));

                for namespace_to_delete in namespaces_to_delete.iter() {
                    match cmd::kubectl::kubectl_exec_delete_namespace(
                        &kubernetes_config_file_path,
                        namespace_to_delete,
                        kubernetes.cloud_provider().credentials_environment_variables(),
                    ) {
                        Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!(
                                "Namespace `{}` deleted successfully.",
                                namespace_to_delete
                            )),
                        )),
                        Err(e) => {
                            if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                                kubernetes.logger().log(EngineEvent::Warning(
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
                    kubernetes.name_with_id(),
                );
                kubernetes.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(message_safe, Some(e.message(ErrorMessageVerbosity::FullDetails))),
                ));
            }
        }

        let message = format!(
            "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
            kubernetes.name(),
            kubernetes.id()
        );

        kubernetes.send_to_customer(&message, &listeners_helper);

        kubernetes
            .logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        // delete custom metrics api to avoid stale namespaces on deletion
        let helm = Helm::new(
            &kubernetes_config_file_path,
            &kubernetes.cloud_provider().credentials_environment_variables(),
        )
        .map_err(|e| to_engine_error(&event_details, e))?;
        let chart = ChartInfo::new_from_release_name("metrics-server", "kube-system");
        helm.uninstall(&chart, &[])
            .map_err(|e| to_engine_error(&event_details, e))?;

        // required to avoid namespace stuck on deletion
        uninstall_cert_manager(
            &kubernetes_config_file_path,
            kubernetes.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            kubernetes.logger(),
        )?;

        kubernetes.logger().log(EngineEvent::Info(
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
                    Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                    )),
                    Err(e) => {
                        let message_safe = format!("Can't delete chart `{}`: {}", &chart.name, e);
                        kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new(message_safe, Some(e.to_string())),
                        ))
                    }
                }
            }
        }

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deleting Qovery managed namespaces".to_string()),
        ));

        for qovery_namespace in qovery_namespaces.iter() {
            let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                &kubernetes_config_file_path,
                qovery_namespace,
                kubernetes.cloud_provider().credentials_environment_variables(),
            );
            match deletion {
                Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!("Namespace {} is fully deleted", qovery_namespace)),
                )),
                Err(e) => {
                    if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                        kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Can't delete namespace {}.", qovery_namespace)),
                        ))
                    }
                }
            }
        }

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Delete all remaining deployed helm applications".to_string()),
        ));

        match helm.list_release(None, &[]) {
            Ok(helm_charts) => {
                for chart in helm_charts {
                    let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                    match helm.uninstall(&chart_info, &[]) {
                        Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                        )),
                        Err(e) => {
                            let message_safe = format!("Error deleting chart `{}`: {}", chart.name, e);
                            kubernetes.logger().log(EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new(message_safe, Some(e.to_string())),
                            ))
                        }
                    }
                }
            }
            Err(e) => {
                let message_safe = "Unable to get helm list";
                kubernetes.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(message_safe.to_string(), Some(e.to_string())),
                ))
            }
        }
    };

    let message = format!("Deleting Kubernetes cluster {}/{}", kubernetes.name(), kubernetes.id());
    kubernetes.send_to_customer(&message, &listeners_helper);
    kubernetes
        .logger()
        .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform destroy".to_string()),
    ));

    match retry::retry(
        Fibonacci::from_millis(60000).take(3),
        || match cmd::terraform::terraform_init_validate_destroy(temp_dir.as_str(), false) {
            Ok(_) => OperationResult::Ok(()),
            Err(e) => OperationResult::Retry(e),
        },
    ) {
        Ok(_) => {
            kubernetes.send_to_customer(
                format!(
                    "Kubernetes cluster {}/{} successfully deleted",
                    kubernetes.name(),
                    kubernetes.id()
                )
                .as_str(),
                &listeners_helper,
            );
            kubernetes.logger().log(EngineEvent::Info(
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
            CommandError::new("Error while trying to perform Terraform destroy".to_string(), Some(msg), None),
        )),
    }
}

fn delete_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete)),
        EventMessage::new_from_safe(format!("{}.delete_error() called.", kubernetes.kind())),
    ));

    Ok(())
}
