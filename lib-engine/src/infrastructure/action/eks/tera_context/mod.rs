use crate::environment::models::domain::ToTerraformString;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::EngineError;
use crate::events::{EventDetails, InfrastructureStep, Stage};
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::cloud_provider::aws::regions::AwsZone;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::dns_provider::DnsProvider;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::aws::Options;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::io_models::context::Features;
use crate::io_models::metrics::MetricsConfiguration;
use crate::io_models::models::{NodeGroupsWithDesiredState, VpcQoveryNetworkMode};
use crate::string::terraform_list_format;
use chrono::Duration as ChronoDuration;
use tera::Context as TeraContext;

mod core_dns_addon;
mod ebs_csi_addon;
mod kube_proxy_addon;
mod vpc_cni_addon;

pub fn eks_tera_context(
    kubernetes: &EKS,
    cloud_provider: &dyn CloudProvider,
    dns_provider: &dyn DnsProvider,
    zones: &[AwsZone],
    node_groups: &[NodeGroupsWithDesiredState],
    options: &Options,
    eks_upgrade_timeout_in_min: ChronoDuration,
    bootstrap_on_fargate: bool,
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
) -> Result<TeraContext, Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::LoadConfiguration));
    let cloud_provider = cloud_provider.downcast_ref();
    let cloud_provider = cloud_provider
        .as_aws()
        .ok_or_else(|| Box::new(EngineError::new_bad_cast(event_details.clone(), "Cloudprovider is not AWS")))?;
    let mut context = TeraContext::new();

    let public_access_cidrs = generate_public_access_cidrs(advanced_settings, qovery_allowed_public_access_cidrs);

    context.insert("public_access_cidrs", &public_access_cidrs);

    context.insert("user_provided_network", &false);
    if let Some(user_network_cfg) = &options.user_provided_network {
        context.insert("user_provided_network", &true);

        context.insert("documentdb_subnets_zone_a_ids", &user_network_cfg.documentdb_subnets_zone_a_ids);
        context.insert("documentdb_subnets_zone_b_ids", &user_network_cfg.documentdb_subnets_zone_b_ids);
        context.insert("documentdb_subnets_zone_c_ids", &user_network_cfg.documentdb_subnets_zone_c_ids);

        context.insert(
            "elasticache_subnets_zone_a_ids",
            &user_network_cfg.elasticache_subnets_zone_a_ids,
        );
        context.insert(
            "elasticache_subnets_zone_b_ids",
            &user_network_cfg.elasticache_subnets_zone_b_ids,
        );
        context.insert(
            "elasticache_subnets_zone_c_ids",
            &user_network_cfg.elasticache_subnets_zone_c_ids,
        );

        context.insert("rds_subnets_zone_a_ids", &user_network_cfg.rds_subnets_zone_a_ids);
        context.insert("rds_subnets_zone_b_ids", &user_network_cfg.rds_subnets_zone_b_ids);
        context.insert("rds_subnets_zone_c_ids", &user_network_cfg.rds_subnets_zone_c_ids);

        context.insert("aws_vpc_eks_id", &user_network_cfg.aws_vpc_eks_id);

        context.insert("eks_subnets_zone_a_ids", &user_network_cfg.eks_subnets_zone_a_ids);
        context.insert("eks_subnets_zone_b_ids", &user_network_cfg.eks_subnets_zone_b_ids);
        context.insert("eks_subnets_zone_c_ids", &user_network_cfg.eks_subnets_zone_c_ids);

        context.insert(
            "eks_karpenter_fargate_subnets_zone_a_ids",
            &user_network_cfg.eks_karpenter_fargate_subnets_zone_a_ids,
        );
        context.insert(
            "eks_karpenter_fargate_subnets_zone_b_ids",
            &user_network_cfg.eks_karpenter_fargate_subnets_zone_b_ids,
        );
        context.insert(
            "eks_karpenter_fargate_subnets_zone_c_ids",
            &user_network_cfg.eks_karpenter_fargate_subnets_zone_c_ids,
        );
    }

    let format_ips =
        |ips: &Vec<String>| -> Vec<String> { ips.iter().map(|ip| format!("\"{ip}\"")).collect::<Vec<_>>() };

    let aws_zones = zones
        .iter()
        .map(|zone| zone.to_terraform_format_string())
        .collect::<Vec<_>>();

    let mut eks_zone_a_subnet_blocks_private = format_ips(&options.eks_zone_a_subnet_blocks);
    let mut eks_zone_b_subnet_blocks_private = format_ips(&options.eks_zone_b_subnet_blocks);
    let mut eks_zone_c_subnet_blocks_private = format_ips(&options.eks_zone_c_subnet_blocks);

    if let Some(nginx_controller_log_format_upstream) =
        &kubernetes.advanced_settings().nginx_controller_log_format_upstream
    {
        context.insert("nginx_controller_log_format_upstream", &nginx_controller_log_format_upstream);
    }

    if let Some(nginx_controller_http_snippet) = &kubernetes.advanced_settings().nginx_controller_http_snippet {
        context.insert(
            "nginx_controller_http_snippet",
            &nginx_controller_http_snippet.to_model().get_snippet_value(),
        );
    }

    if let Some(nginx_controller_server_snippet) = &kubernetes.advanced_settings().nginx_controller_server_snippet {
        context.insert(
            "nginx_controller_server_snippet",
            &nginx_controller_server_snippet.to_model().get_snippet_value(),
        );
    }

    context.insert(
        "nginx_controller_enable_compression",
        &kubernetes.advanced_settings().nginx_controller_enable_compression,
    );
    context.insert(
        "aws_enable_vpc_flow_logs",
        &kubernetes.advanced_settings().aws_vpc_enable_flow_logs,
    );
    context.insert(
        "vpc_flow_logs_retention_days",
        &kubernetes.advanced_settings().aws_vpc_flow_logs_retention_days,
    );
    context.insert(
        "s3_flow_logs_bucket_name",
        format!("qovery-vpc-flow-logs-{}", kubernetes.short_id()).as_str(),
    );

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
            let max_subnet_zone_c = check_odd_subnets(event_details, "c", &ec2_zone_c_subnet_blocks_private)?;

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

    let fargate_profile_zone_a_subnet_blocks = match options.fargate_profile_zone_a_subnet_blocks.is_empty() {
        true => format_ips(&vec!["10.0.166.0/24".to_string()]),
        false => format_ips(&options.fargate_profile_zone_a_subnet_blocks),
    };
    let fargate_profile_zone_b_subnet_blocks = match options.fargate_profile_zone_b_subnet_blocks.is_empty() {
        true => format_ips(&vec!["10.0.168.0/24".to_string()]),
        false => format_ips(&options.fargate_profile_zone_b_subnet_blocks),
    };
    let fargate_profile_zone_c_subnet_blocks = match options.fargate_profile_zone_c_subnet_blocks.is_empty() {
        true => format_ips(&vec!["10.0.170.0/24".to_string()]),
        false => format_ips(&options.fargate_profile_zone_c_subnet_blocks),
    };
    let eks_zone_a_nat_gw_for_fargate_subnet_blocks_public =
        match options.eks_zone_a_nat_gw_for_fargate_subnet_blocks_public.is_empty() {
            true => format_ips(&vec!["10.0.132.0/22".to_string()]),
            false => format_ips(&options.eks_zone_a_nat_gw_for_fargate_subnet_blocks_public),
        };

    let region_cluster_id = format!("{}-{}", kubernetes.region(), kubernetes.short_id());
    let vpc_cidr_block = options.vpc_cidr_block.clone();
    let cloudwatch_eks_log_group = format!("/aws/eks/{}/cluster", kubernetes.cluster_name());
    let eks_cidr_subnet = options.eks_cidr_subnet.clone();
    let ec2_cidr_subnet = options.ec2_cidr_subnet.clone();

    let qovery_api_url = options.qovery_api_url.clone();
    let rds_cidr_subnet = options.rds_cidr_subnet.clone();
    let documentdb_cidr_subnet = options.documentdb_cidr_subnet.clone();
    let elasticache_cidr_subnet = options.elasticache_cidr_subnet.clone();

    context.insert(
        "object_storage_enable_logging",
        &kubernetes.advanced_settings().object_storage_enable_logging,
    );

    // Qovery
    context.insert("organization_id", kubernetes.context.organization_short_id());
    context.insert("organization_long_id", &kubernetes.context.organization_long_id().to_string());
    context.insert("qovery_api_url", &qovery_api_url);

    context.insert("test_cluster", &kubernetes.context().is_test_cluster());

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
    let managed_dns_list = vec![dns_provider.name()];
    let managed_dns_domains_helm_format = vec![dns_provider.domain().to_string()];
    let managed_dns_domains_root_helm_format = vec![dns_provider.domain().root_domain().to_string()];
    let managed_dns_domains_terraform_format = terraform_list_format(vec![dns_provider.domain().to_string()]);
    let managed_dns_domains_root_terraform_format =
        terraform_list_format(vec![dns_provider.domain().root_domain().to_string()]);
    let managed_dns_resolvers_terraform_format =
        terraform_list_format(dns_provider.resolvers().iter().map(|x| x.clone().to_string()).collect());

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

    // add specific DNS fields
    dns_provider.insert_into_teracontext(&mut context);

    context.insert("dns_email_report", &options.tls_email_report);

    // TLS
    context.insert(
        "acme_server_url",
        LetsEncryptConfig::acme_url_for_given_usage(kubernetes.context().is_test_cluster()).as_str(),
    );

    // Other Kubernetes
    context.insert("kubernetes_cluster_name", &kubernetes.cluster_name());
    context.insert("enable_cluster_autoscaler", &true);

    // AWS
    context.insert("aws_access_key", cloud_provider.aws_credentials().access_key_id());
    context.insert("aws_secret_key", cloud_provider.aws_credentials().secret_access_key());
    context.insert("aws_session_token", &cloud_provider.aws_credentials().session_token());

    // Karpenter
    context.insert("enable_karpenter", &kubernetes.is_karpenter_enabled());
    context.insert("bootstrap_on_fargate", &bootstrap_on_fargate);
    context.insert("fargate_profile_zone_a_subnet_blocks", &fargate_profile_zone_a_subnet_blocks);
    context.insert("fargate_profile_zone_b_subnet_blocks", &fargate_profile_zone_b_subnet_blocks);
    context.insert("fargate_profile_zone_c_subnet_blocks", &fargate_profile_zone_c_subnet_blocks);
    context.insert(
        "eks_zone_a_nat_gw_for_fargate_subnet_blocks_public",
        &eks_zone_a_nat_gw_for_fargate_subnet_blocks_public,
    );

    // AWS S3 tfstate storage
    context.insert(
        "aws_access_key_tfstates_account",
        match cloud_provider.terraform_state_credentials() {
            Some(x) => x.access_key_id.as_str(),
            None => "",
        },
    );

    context.insert(
        "aws_secret_key_tfstates_account",
        match cloud_provider.terraform_state_credentials() {
            Some(x) => x.secret_access_key.as_str(),
            None => "",
        },
    );
    context.insert(
        "aws_region_tfstates_account",
        match cloud_provider.terraform_state_credentials() {
            Some(x) => x.region.as_str(),
            None => "",
        },
    );
    context.insert(
        "aws_terraform_backend_bucket",
        match cloud_provider.terraform_state_credentials() {
            Some(x) => x.s3_bucket.as_str(),
            None => "",
        },
    );
    context.insert(
        "aws_terraform_backend_dynamodb_table",
        match cloud_provider.terraform_state_credentials() {
            Some(x) => x.dynamodb_table.as_str(),
            None => "",
        },
    );

    context.insert("aws_region", &kubernetes.region());
    context.insert("vpc_cidr_block", &vpc_cidr_block);
    context.insert("vpc_custom_routing_table", &options.vpc_custom_routing_table);
    context.insert("s3_kubeconfig_bucket", &format!("qovery-kubeconfigs-{}", kubernetes.short_id()));

    // AWS - EKS
    context.insert("aws_availability_zones", &aws_zones);
    context.insert("eks_cidr_subnet", &eks_cidr_subnet);
    context.insert("ec2_cidr_subnet", &ec2_cidr_subnet);
    context.insert("kubernetes_cluster_name", kubernetes.name());
    context.insert("kubernetes_cluster_id", kubernetes.short_id());
    context.insert("kubernetes_cluster_long_id", kubernetes.context().cluster_long_id());
    context.insert("eks_region_cluster_id", region_cluster_id.as_str());
    context.insert("eks_worker_nodes", &node_groups);
    context.insert("ec2_zone_a_subnet_blocks_private", &ec2_zone_a_subnet_blocks_private);
    context.insert("ec2_zone_b_subnet_blocks_private", &ec2_zone_b_subnet_blocks_private);
    context.insert("ec2_zone_c_subnet_blocks_private", &ec2_zone_c_subnet_blocks_private);
    context.insert("eks_zone_a_subnet_blocks_private", &eks_zone_a_subnet_blocks_private);
    context.insert("eks_zone_b_subnet_blocks_private", &eks_zone_b_subnet_blocks_private);
    context.insert("eks_zone_c_subnet_blocks_private", &eks_zone_c_subnet_blocks_private);
    context.insert("eks_masters_version", &kubernetes.version().to_string());
    context.insert("eks_workers_version", &kubernetes.version().to_string());
    context.insert("ec2_masters_version", &kubernetes.version().to_string());
    context.insert("ec2_workers_version", &kubernetes.version().to_string());
    context.insert("k3s_version", &kubernetes.version().to_string());

    context.insert("eks_upgrade_timeout_in_min", &eks_upgrade_timeout_in_min.num_minutes());

    // TODO(ENG-1456): remove condition when migration is done
    if let (Some(suffix), Some(patch)) = (kubernetes.version().suffix(), kubernetes.version().patch()) {
        if suffix.as_ref() == "+k3s1" && patch == &8 {
            context.insert("is_old_k3s_version", &true);
            context.insert("ec2_port", &9876.to_string());
        }
    }

    if let Some(port) = options.ec2_exposed_port {
        context.insert("ec2_port", &port.to_string());
    }

    // Encrypt cluster secrets with a KMS key
    if !kubernetes
        .advanced_settings()
        .aws_eks_encrypt_secrets_kms_key_arn
        .is_empty()
    {
        context.insert(
            "aws_eks_encrypt_secrets_kms_key_arn",
            &kubernetes.advanced_settings().aws_eks_encrypt_secrets_kms_key_arn,
        );
    }

    context.insert("cloudwatch_eks_log_group", &cloudwatch_eks_log_group);
    context.insert(
        "aws_cloudwatch_eks_logs_retention_days",
        &kubernetes.advanced_settings().aws_cloudwatch_eks_logs_retention_days,
    );

    // AWS - EKS/EC2 Metadata
    // https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/configuring-IMDS-existing-instances.html
    context.insert(
        "ec2_metadata_imds_version",
        &kubernetes.advanced_settings().aws_eks_ec2_metadata_imds,
    );

    // AWS - RDS
    context.insert("rds_cidr_subnet", &rds_cidr_subnet);
    context.insert("rds_zone_a_subnet_blocks", &rds_zone_a_subnet_blocks);
    context.insert("rds_zone_b_subnet_blocks", &rds_zone_b_subnet_blocks);
    context.insert("rds_zone_c_subnet_blocks", &rds_zone_c_subnet_blocks);
    context.insert(
        "database_postgresql_deny_any_access",
        &kubernetes.advanced_settings().database_postgresql_deny_any_access,
    );
    context.insert(
        "database_postgresql_allowed_cidrs",
        &format_ips(&kubernetes.advanced_settings().database_postgresql_allowed_cidrs),
    );
    context.insert(
        "database_mysql_deny_any_access",
        &kubernetes.advanced_settings().database_mysql_deny_any_access,
    );
    context.insert(
        "database_mysql_allowed_cidrs",
        &format_ips(&kubernetes.advanced_settings().database_mysql_allowed_cidrs),
    );

    // AWS - DocumentDB
    context.insert("documentdb_cidr_subnet", &documentdb_cidr_subnet);
    context.insert("documentdb_zone_a_subnet_blocks", &documentdb_zone_a_subnet_blocks);
    context.insert("documentdb_zone_b_subnet_blocks", &documentdb_zone_b_subnet_blocks);
    context.insert("documentdb_zone_c_subnet_blocks", &documentdb_zone_c_subnet_blocks);
    context.insert(
        "database_mongodb_deny_any_access",
        &kubernetes.advanced_settings().database_mongodb_deny_any_access,
    );
    context.insert(
        "database_mongodb_allowed_cidrs",
        &format_ips(&kubernetes.advanced_settings().database_mongodb_allowed_cidrs),
    );

    // AWS - Elasticache
    context.insert("elasticache_cidr_subnet", &elasticache_cidr_subnet);
    context.insert("elasticache_zone_a_subnet_blocks", &elasticache_zone_a_subnet_blocks);
    context.insert("elasticache_zone_b_subnet_blocks", &elasticache_zone_b_subnet_blocks);
    context.insert("elasticache_zone_c_subnet_blocks", &elasticache_zone_c_subnet_blocks);
    context.insert(
        "database_redis_deny_any_access",
        &kubernetes.advanced_settings().database_redis_deny_any_access,
    );
    context.insert(
        "database_redis_allowed_cidrs",
        &format_ips(&kubernetes.advanced_settings().database_redis_allowed_cidrs),
    );

    // grafana credentials
    context.insert("grafana_admin_user", options.grafana_admin_user.as_str());
    context.insert("grafana_admin_password", options.grafana_admin_password.as_str());

    // qovery
    context.insert("qovery_api_url", options.qovery_api_url.as_str());
    context.insert("qovery_ssh_key", options.qovery_ssh_key.as_str());
    // AWS support only 1 ssh key
    let user_ssh_key: Option<&str> = options.user_ssh_keys.first().map(|x| x.as_str());
    context.insert("user_ssh_key", user_ssh_key.unwrap_or_default());

    // Advanced settings
    context.insert(
        "registry_image_retention_time",
        &kubernetes.advanced_settings().registry_image_retention_time_sec,
    );
    context.insert(
        "resource_expiration_in_seconds",
        &kubernetes.advanced_settings().pleco_resources_ttl,
    );
    context.insert(
        "aws_iam_user_mapper_sso_enabled",
        &kubernetes.advanced_settings().aws_iam_user_mapper_sso_enabled,
    );
    context.insert(
        "aws_iam_user_mapper_sso_role_arn",
        &kubernetes.advanced_settings().aws_iam_user_mapper_sso_role_arn,
    );
    context.insert(
        "loki_log_retention_in_week",
        &kubernetes.advanced_settings().loki_log_retention_in_week,
    );

    // EKS Addons
    // CNI
    context.insert(
        "eks_addon_vpc_cni",
        &(match &options.aws_addon_cni_version_override {
            None => vpc_cni_addon::AwsVpcCniAddon::new_from_k8s_version(kubernetes.version()),

            Some(overridden_version) => vpc_cni_addon::AwsVpcCniAddon::new_with_overridden_version(overridden_version),
        }),
    );
    // Kube-proxy
    context.insert(
        "eks_addon_kube_proxy",
        &(match &options.aws_addon_kube_proxy_version_override {
            None => kube_proxy_addon::AwsKubeProxyAddon::new_from_k8s_version(kubernetes.version()),
            Some(overridden_version) => {
                kube_proxy_addon::AwsKubeProxyAddon::new_with_overridden_version(overridden_version)
            }
        }),
    );
    // EBS CSI
    context.insert(
        "eks_addon_ebs_csi",
        &(match &options.aws_addon_ebs_csi_version_override {
            None => ebs_csi_addon::AwsEbsCsiAddon::new_from_k8s_version(kubernetes.version()),
            Some(overridden_version) => ebs_csi_addon::AwsEbsCsiAddon::new_with_overridden_version(overridden_version),
        }),
    );
    // COREDNS
    context.insert(
        "eks_addon_coredns",
        &(match &options.aws_addon_coredns_version_override {
            None => core_dns_addon::AwsCoreDnsAddon::new_from_k8s_version(kubernetes.version()),
            Some(overridden_version) => {
                core_dns_addon::AwsCoreDnsAddon::new_with_overridden_version(overridden_version)
            }
        }),
    );

    // Move thanos pods to the stable node pool when the redundancy is disabled
    let thanos_on_stable = options.metrics_parameters.as_ref().is_some_and(|mp| {
        matches!(
            mp.config,
            MetricsConfiguration::MetricsInstalledByQovery {
                enable_redundancy: Some(false),
                ..
            }
        )
    });

    context.insert("thanos_nodepool_stable", &thanos_on_stable);

    Ok(context)
}

fn generate_public_access_cidrs(
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
) -> Vec<String> {
    match (
        advanced_settings.qovery_static_ip_mode.unwrap_or(false),
        qovery_allowed_public_access_cidrs,
    ) {
        (true, Some(qovery_allowed_public_access_cidrs)) if !qovery_allowed_public_access_cidrs.is_empty() => {
            match &advanced_settings.k8s_api_allowed_public_access_cidrs {
                Some(k8s_api_allowed_public_access_cidrs) => [
                    qovery_allowed_public_access_cidrs.clone(),
                    k8s_api_allowed_public_access_cidrs.clone(),
                ]
                .concat(),
                None => qovery_allowed_public_access_cidrs.clone(),
            }
        }
        _ => vec!["0.0.0.0/0".to_string()],
    }
}

/// divide by 2 the total number of subnet to get the exact same number as private and public
fn check_odd_subnets(
    event_details: EventDetails,
    zone_name: &str,
    subnet_block: &[String],
) -> Result<usize, Box<EngineError>> {
    if subnet_block.len() % 2 == 1 {
        return Err(Box::new(EngineError::new_subnets_count_is_not_even(
            event_details,
            zone_name.to_string(),
            subnet_block.len(),
        )));
    }

    Ok(subnet_block.len() / 2)
}

#[cfg(test)]
mod tests {
    use super::generate_public_access_cidrs;
    use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;

    #[test]
    fn test_public_access_cidrs_with_any_parameters_set() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: None,
            k8s_api_allowed_public_access_cidrs: None,
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = None;

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs);

        assert_eq!(cidrs, vec!["0.0.0.0/0".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_disabled() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(false),
            k8s_api_allowed_public_access_cidrs: None,
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = None;

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs);

        assert_eq!(cidrs, vec!["0.0.0.0/0".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_disabled_and_qovey_cidr() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(false),
            k8s_api_allowed_public_access_cidrs: None,
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = Some(vec!["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]);

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs.as_ref());

        assert_eq!(cidrs, vec!["0.0.0.0/0".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_enabled_but_without_qovery_cidr() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(true),
            k8s_api_allowed_public_access_cidrs: Some(vec!["1.1.1.1/32".to_string()]),
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = Some(vec![]);

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs.as_ref());

        assert_eq!(cidrs, vec!["0.0.0.0/0".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_enabled() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(true),
            k8s_api_allowed_public_access_cidrs: Some(vec![]),
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = Some(vec!["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]);

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs.as_ref());

        assert_eq!(cidrs, vec!["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_enabled_and_custom_cidr() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(true),
            k8s_api_allowed_public_access_cidrs: Some(vec!["1.1.1.4/32".to_string()]),
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = Some(vec!["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]);

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs.as_ref());

        assert_eq!(
            cidrs,
            vec![
                "1.1.1.2/32".to_string(),
                "1.1.1.3/32".to_string(),
                "1.1.1.4/32".to_string()
            ]
        );
    }
}
