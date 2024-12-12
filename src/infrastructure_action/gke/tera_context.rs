use crate::cloud_provider::gcp::kubernetes::{Gke, VpcMode};
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::models::VpcQoveryNetworkMode;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::infrastructure_action::ToInfraTeraContext;
use crate::io_models::context::Features;
use crate::models::third_parties::LetsEncryptConfig;
use crate::models::types::Percentage;
use crate::models::ToCloudProviderFormat;
use crate::string::terraform_list_format;
use tera::Context as TeraContext;
use time::format_description;

impl ToInfraTeraContext for Gke {
    fn to_infra_tera_context(&self, infra_ctx: &InfrastructureContext) -> Result<TeraContext, Box<EngineError>> {
        gke_tera_context(self, infra_ctx)
    }
}

fn gke_tera_context(cluster: &Gke, infra_ctx: &InfrastructureContext) -> Result<TeraContext, Box<EngineError>> {
    let mut context = TeraContext::new();

    // Qovery
    context.insert("organization_id", infra_ctx.cloud_provider().organization_id());
    context.insert(
        "organization_long_id",
        &infra_ctx.cloud_provider().organization_long_id().to_string(),
    );
    context.insert("object_storage_kubeconfig_bucket", &cluster.kubeconfig_bucket_name());
    context.insert("object_storage_logs_bucket", &cluster.logs_bucket_name());
    // Qovery features
    context.insert(
        "log_history_enabled",
        &cluster.context.is_feature_enabled(&Features::LogsHistory),
    );
    context.insert(
        "metrics_history_enabled",
        &cluster.context.is_feature_enabled(&Features::MetricsHistory),
    );

    // Advanced settings
    context.insert(
        "resource_expiration_in_seconds",
        &cluster.advanced_settings().pleco_resources_ttl,
    );

    // Kubernetes
    context.insert("test_cluster", &cluster.context.is_test_cluster());
    context.insert("kubernetes_cluster_long_id", &cluster.long_id);
    context.insert("kubernetes_cluster_id", cluster.short_id());
    context.insert("kubernetes_cluster_name", cluster.cluster_name().as_str());
    context.insert("kubernetes_cluster_version", &cluster.version.to_string());
    context.insert("qovery_api_url", cluster.options.qovery_api_url.as_str());

    // GCP
    // credentials
    context.insert(
        "gcp_json_credentials_raw",
        &cluster.options.gcp_json_credentials.r#type.to_string(),
    );
    context.insert(
        "gcp_json_credentials_type",
        &cluster.options.gcp_json_credentials.r#type.to_string(),
    );
    context.insert(
        "gcp_json_credentials_private_key_id",
        &cluster.options.gcp_json_credentials.private_key_id.to_string(),
    );
    context.insert(
        "gcp_json_credentials_private_key",
        &cluster
            .options
            .gcp_json_credentials
            .private_key
            .as_str()
            .escape_default() // escape new lines to have \n instead
            .to_string(),
    );
    context.insert(
        "gcp_json_credentials_client_email",
        &cluster.options.gcp_json_credentials.client_email.to_string(),
    );
    context.insert(
        "gcp_json_credentials_client_id",
        &cluster.options.gcp_json_credentials.client_id.to_string(),
    );
    context.insert(
        "gcp_json_credentials_auth_uri",
        cluster.options.gcp_json_credentials.auth_uri.as_str(),
    );
    context.insert(
        "gcp_json_credentials_token_uri",
        cluster.options.gcp_json_credentials.token_uri.as_str(),
    );
    context.insert(
        "gcp_json_credentials_auth_provider_x509_cert_url",
        cluster
            .options
            .gcp_json_credentials
            .auth_provider_x509_cert_url
            .as_str(),
    );
    context.insert(
        "gcp_json_credentials_client_x509_cert_url",
        cluster.options.gcp_json_credentials.client_x509_cert_url.as_str(),
    );
    context.insert(
        "gcp_json_credentials_universe_domain",
        &cluster.options.gcp_json_credentials.universe_domain.to_string(),
    );
    context.insert("gcp_project_id", cluster.options.gcp_json_credentials.project_id.as_str());
    context.insert("gcp_region", &cluster.region.to_cloud_provider_format());
    context.insert(
        "gcp_zones",
        &cluster
            .region
            .zones()
            .iter()
            .map(|z| z.to_cloud_provider_format())
            .collect::<Vec<&str>>(),
    );
    let rfc3339_format = format_description::parse("[hour]:[minute]").unwrap_or_default();
    context.insert(
        "cluster_maintenance_start_time",
        &cluster
            .options
            .cluster_maintenance_start_time
            .format(&rfc3339_format)
            .unwrap_or_default(),
    ); // RFC3339 https://www.ietf.org/rfc/rfc3339.txt
    let cluster_maintenance_end_time = match &cluster.options.cluster_maintenance_end_time {
        Some(t) => t.format(&rfc3339_format).unwrap_or_default(),
        None => "".to_string(),
    };
    context.insert("cluster_maintenance_end_time", cluster_maintenance_end_time.as_str()); // RFC3339 https://www.ietf.org/rfc/rfc3339.txt

    // Network
    // VPC
    match &cluster.options.vpc_qovery_network_mode {
        Some(mode) => {
            context.insert(
                "cluster_is_private",
                &match mode {
                    VpcQoveryNetworkMode::WithNatGateways => true,
                    VpcQoveryNetworkMode::WithoutNatGateways => false,
                },
            ); // cluster is made private when requires static IP
            context.insert("vpc_network_mode", &mode.to_string());
        }
        None => {
            context.insert("cluster_is_private", &false); // cluster is public unless requires static IP
            context.insert(
                "vpc_network_mode",
                VpcQoveryNetworkMode::WithoutNatGateways.to_string().as_str(),
            );
        }
    }

    match &cluster.options.vpc_mode {
        VpcMode::Automatic {
            custom_cluster_ipv4_cidr_block,
            custom_services_ipv4_cidr_block,
        } => {
            // if automatic, Qovery to create a new VPC for the cluster
            context.insert("vpc_use_existing", &false);
            context.insert("vpc_name", cluster.cluster_name().as_str());
            context.insert("subnetwork", cluster.cluster_name().as_str());
            context.insert(
                "cluster_ipv4_cidr_block",
                &custom_cluster_ipv4_cidr_block
                    .map(|net| net.to_string())
                    .unwrap_or_default(),
            );
            context.insert(
                "services_ipv4_cidr_block",
                &custom_services_ipv4_cidr_block
                    .map(|net| net.to_string())
                    .unwrap_or_default(),
            );
            context.insert("network_project_id", "");
            context.insert("ip_range_pods", "");
            context.insert("ip_range_services", "");
            context.insert("additional_ip_range_pods", "");

            // VPC log flow (won't be set for user provided VPC)
            context.insert("vpc_enable_flow_logs", &cluster.advanced_settings.gcp_vpc_enable_flow_logs);
            context.insert(
                "vpc_flow_logs_sampling",
                &cluster
                    .advanced_settings
                    .gcp_vpc_flow_logs_sampling
                    .as_ref()
                    .unwrap_or(&Percentage::min())
                    .as_f64(),
            );
        }
        VpcMode::UserNetworkConfig {
            vpc_project_id,
            vpc_name,
            subnetwork_name,
            ip_range_pods_name,
            additional_ip_range_pods_names,
            ip_range_services_name,
        } => {
            // If VPC is provided by client, then reuse it without creating a new VPC for the cluster
            context.insert("vpc_use_existing", &true);
            context.insert(
                "network_project_id",
                vpc_project_id
                    .as_ref()
                    .unwrap_or(&cluster.options.gcp_json_credentials.project_id), // If no project set, use the current one
            );
            context.insert("vpc_name", &vpc_name);
            context.insert("subnetwork", &subnetwork_name);
            context.insert("cluster_ipv4_cidr_block", "");
            context.insert("services_ipv4_cidr_block", "");
            context.insert(
                "ip_range_pods",
                match ip_range_pods_name {
                    None => "",
                    Some(name) => name.as_str(),
                },
            );
            context.insert(
                "ip_range_services",
                match ip_range_services_name {
                    None => "",
                    Some(name) => name.as_str(),
                },
            );
            context.insert(
                "additional_ip_range_pods",
                &additional_ip_range_pods_names.clone().unwrap_or_default(),
            );

            // VPC log flow (won't be set for user provided VPC)
            context.insert("vpc_enable_flow_logs", &false);
            context.insert("vpc_flow_logs_sampling", &Percentage::min().as_f64());
        }
    }

    // AWS S3 tfstates storage
    context.insert(
        "aws_access_key_tfstates_account",
        match infra_ctx.cloud_provider().terraform_state_credentials() {
            Some(x) => x.access_key_id.as_str(),
            None => "",
        },
    );
    context.insert(
        "aws_secret_key_tfstates_account",
        match infra_ctx.cloud_provider().terraform_state_credentials() {
            Some(x) => x.secret_access_key.as_str(),
            None => "",
        },
    );
    context.insert(
        "aws_region_tfstates_account",
        match infra_ctx.cloud_provider().terraform_state_credentials() {
            Some(x) => x.region.as_str(),
            None => "",
        },
    );
    context.insert(
        "aws_terraform_backend_bucket",
        match infra_ctx.cloud_provider().terraform_state_credentials() {
            Some(x) => x.s3_bucket.as_str(),
            None => "",
        },
    );
    context.insert(
        "aws_terraform_backend_dynamodb_table",
        match infra_ctx.cloud_provider().terraform_state_credentials() {
            Some(x) => x.dynamodb_table.as_str(),
            None => "",
        },
    );

    // DNS
    let managed_dns_list = vec![infra_ctx.dns_provider().name()];
    let managed_dns_domains_helm_format = vec![infra_ctx.dns_provider().domain().to_string()];
    let managed_dns_domains_root_helm_format = vec![infra_ctx.dns_provider().domain().root_domain().to_string()];
    let managed_dns_domains_terraform_format =
        terraform_list_format(vec![infra_ctx.dns_provider().domain().to_string()]);
    let managed_dns_domains_root_terraform_format =
        terraform_list_format(vec![infra_ctx.dns_provider().domain().root_domain().to_string()]);
    let managed_dns_resolvers_terraform_format = terraform_list_format(
        infra_ctx
            .dns_provider()
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

    // add specific DNS fields
    infra_ctx.dns_provider().insert_into_teracontext(&mut context);

    context.insert("dns_email_report", &cluster.options.tls_email_report);

    // TLS
    context.insert(
        "acme_server_url",
        LetsEncryptConfig::new(cluster.options.tls_email_report.to_string(), cluster.context.is_test_cluster())
            .acme_url()
            .as_str(),
    );

    // grafana credentials
    context.insert("grafana_admin_user", cluster.options.grafana_admin_user.as_str());
    context.insert("grafana_admin_password", cluster.options.grafana_admin_password.as_str());

    if let Some(nginx_controller_log_format_upstream) =
        &cluster.advanced_settings().nginx_controller_log_format_upstream
    {
        context.insert("nginx_controller_log_format_upstream", &nginx_controller_log_format_upstream);
    }

    Ok(context)
}
