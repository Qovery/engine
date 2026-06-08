use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::gcp::GcpCredentials;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::environment::models::types::Percentage;
use crate::errors::CommandError as EngineCommandError;
use crate::errors::EngineError;
use crate::events::{InfrastructureStep, Stage::Infrastructure};
use crate::infrastructure::action::ToInfraTeraContext;
use crate::infrastructure::action::utils::{generate_public_access_cidrs, is_api_access_restricted};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::external_secrets::gcp_secrets_manager_authentication::GcpAuthenticationMode;
use crate::infrastructure::models::external_secrets::{SecretsManagerAccess, SecretsManagerConnection};
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::gcp::{Gke, VpcMode};
use crate::io_models::context::Features;
use crate::io_models::models::VpcQoveryNetworkMode;
use crate::string::terraform_list_format;
use serde::Serialize;
use std::io::Write;
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
    context.insert(
        "qovery_deployed_with_engine_version",
        &infra_ctx.context().engine_version().to_string(),
    );
    context.insert("organization_id", infra_ctx.context().organization_short_id());
    context.insert("organization_long_id", &infra_ctx.context().organization_long_id().to_string());
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
    context.insert("enable_keda", &cluster.is_keda_enabled());

    // Advanced settings
    context.insert(
        "resource_expiration_in_seconds",
        &cluster.advanced_settings().pleco_resources_ttl,
    );
    context.insert(
        "master_authorized_networks",
        &compute_master_authorized_networks(
            cluster.advanced_settings(),
            cluster.qovery_allowed_public_access_cidrs.as_ref(),
        ),
    );

    // thanos
    context.insert("thanos_gcs_bucket_name", &cluster.prometheus_bucket_name());

    // Kubernetes
    context.insert("test_cluster", &cluster.context.is_test_cluster());
    context.insert("kubernetes_cluster_long_id", &cluster.long_id);
    context.insert("kubernetes_cluster_id", cluster.short_id());
    context.insert("kubernetes_cluster_name", cluster.cluster_name().as_str());
    context.insert("kubernetes_cluster_version", &cluster.version.to_string());
    context.insert("qovery_api_url", cluster.options.qovery_api_url.as_str());

    // GCP
    // credentials
    match &cluster.credentials {
        GcpCredentials::ServiceAccount(credentials) => {
            context.insert("gcp_json_credentials_raw", &credentials.r#type.to_string());
            context.insert("gcp_json_credentials_type", &credentials.r#type.to_string());
            context.insert("gcp_json_credentials_private_key_id", &credentials.private_key_id.to_string());
            context.insert(
                "gcp_json_credentials_private_key",
                &credentials
                    .private_key
                    .as_str()
                    .escape_default() // escape new lines to have \n instead
                    .to_string(),
            );
            context.insert("gcp_json_credentials_client_email", &credentials.client_email.to_string());
            context.insert("gcp_json_credentials_client_id", &credentials.client_id.to_string());
            context.insert("gcp_json_credentials_auth_uri", credentials.auth_uri.as_str());
            context.insert("gcp_json_credentials_token_uri", credentials.token_uri.as_str());
            context.insert(
                "gcp_json_credentials_auth_provider_x509_cert_url",
                credentials.auth_provider_x509_cert_url.as_str(),
            );
            context.insert(
                "gcp_json_credentials_client_x509_cert_url",
                credentials.client_x509_cert_url.as_str(),
            );
            context.insert("gcp_json_credentials_universe_domain", &credentials.universe_domain.to_string());
        }
        GcpCredentials::AccessToken(_) => {
            context.insert("gcp_json_credentials_raw", "");
            context.insert("gcp_json_credentials_type", "");
            context.insert("gcp_json_credentials_private_key_id", "");
            context.insert("gcp_json_credentials_private_key", "");
            context.insert("gcp_json_credentials_client_email", "");
            context.insert("gcp_json_credentials_client_id", "");
            context.insert("gcp_json_credentials_auth_uri", "");
            context.insert("gcp_json_credentials_token_uri", "");
            context.insert("gcp_json_credentials_auth_provider_x509_cert_url", "");
            context.insert("gcp_json_credentials_client_x509_cert_url", "");
            context.insert("gcp_json_credentials_universe_domain", "");
        }
    }

    // For WIF/AccessToken credentials, gcloud CLI needs a token file for Terraform local-exec
    // provisioners (e.g. networks.j2.tf). The persisted kubeconfig must not embed this token
    // because it expires; it uses `qovery cluster get-token` instead.
    let (gcp_wif_credentials, gcp_access_token_file_path) = match &cluster.credentials {
        GcpCredentials::ServiceAccount(_) => (false, String::new()),
        GcpCredentials::AccessToken(credentials) => {
            let token_file_path = cluster.temp_dir.join("gcp-access-token");
            let file_path = match std::fs::File::create(&token_file_path)
                .and_then(|mut f| f.write_all(credentials.access_token.as_bytes()))
            {
                Ok(_) => token_file_path.to_string_lossy().into_owned(),
                Err(e) => {
                    return Err(Box::new(EngineError::new_cannot_get_cluster_error(
                        cluster.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
                        EngineCommandError::new_from_safe_message(format!(
                            "Cannot write GCP access token file for Terraform local-exec: {e}"
                        )),
                    )));
                }
            };
            (true, file_path)
        }
    };
    context.insert("gcp_wif_credentials", &gcp_wif_credentials);
    context.insert("gcp_access_token_file", &gcp_access_token_file_path);

    context.insert("gcp_project_id", cluster.credentials.project_id());
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

    // NAT Gateway static egress IPs: only meaningful when NAT Gateway is enabled.
    let gcp_static_egress_ips_enabled = matches!(
        &cluster.options.vpc_qovery_network_mode,
        Some(VpcQoveryNetworkMode::WithNatGateways)
    ) && cluster
        .options
        .nat_gateway_parameters
        .as_ref()
        .is_some_and(|p| p.gcp_static_ips_enabled());
    let gcp_static_egress_ips_count = cluster
        .options
        .nat_gateway_parameters
        .as_ref()
        .and_then(|p| p.gcp_static_ips_count())
        .unwrap_or(2);
    context.insert("gcp_static_egress_ips_enabled", &gcp_static_egress_ips_enabled);
    context.insert("gcp_static_egress_ips_count", &gcp_static_egress_ips_count);

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
                vpc_project_id.as_deref().unwrap_or(cluster.credentials.project_id()), // If no project set, use the current one
            );
            context.insert("vpc_name", &vpc_name);
            context.insert("subnetwork", subnetwork_name.as_deref().unwrap_or(""));
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

    if let Some(nginx_controller_http_snippet) = &cluster.advanced_settings().nginx_controller_http_snippet {
        context.insert(
            "nginx_controller_http_snippet",
            &nginx_controller_http_snippet.to_model().get_snippet_value(),
        );
    }

    if let Some(nginx_controller_server_snippet) = &cluster.advanced_settings().nginx_controller_server_snippet {
        context.insert(
            "nginx_controller_server_snippet",
            &nginx_controller_server_snippet.to_model().get_snippet_value(),
        );
    }

    context.insert(
        "nginx_controller_enable_compression",
        &cluster.advanced_settings().nginx_controller_enable_compression,
    );

    context.insert("prometheus_enabled", &cluster.options.metrics_parameters.is_some());

    // External Secrets Operator
    let eso_config = compute_secrets_manager_config(&cluster.options.secrets_manager_accesses);
    context.insert("enable_automatic_external_secrets_access", &eso_config.enable_automatic_eso);

    Ok(context)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct MasterAuthorizedNetwork {
    cidr_block: String,
    display_name: String,
}

fn compute_master_authorized_networks(
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
) -> Vec<MasterAuthorizedNetwork> {
    let public_access_cidrs = generate_public_access_cidrs(advanced_settings, qovery_allowed_public_access_cidrs);
    if !is_api_access_restricted(&public_access_cidrs) {
        return Vec::new();
    }

    public_access_cidrs
        .into_iter()
        .map(|cidr| MasterAuthorizedNetwork {
            cidr_block: cidr.clone(),
            display_name: cidr,
        })
        .collect()
}

#[derive(Debug, PartialEq)]
struct SecretsManagerConfig {
    enable_automatic_eso: bool,
}

fn compute_secrets_manager_config(accesses: &[SecretsManagerAccess]) -> SecretsManagerConfig {
    let has_automatic_accesses = accesses.iter().any(|a| {
        if let SecretsManagerConnection::Gcp(conn) = &a.connection {
            conn.authentication_mode == GcpAuthenticationMode::Automatic
        } else {
            false
        }
    });
    SecretsManagerConfig {
        enable_automatic_eso: has_automatic_accesses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_not_configure_master_authorized_networks_when_api_is_unrestricted() {
        let advanced_settings = ClusterAdvancedSettings::default();

        let networks = compute_master_authorized_networks(&advanced_settings, None);

        assert!(networks.is_empty());
    }

    #[test]
    fn should_configure_master_authorized_networks_when_api_is_restricted() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(true),
            k8s_api_allowed_public_access_cidrs: Some(vec!["203.0.113.10/32".to_string()]),
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = vec!["198.51.100.5/32".to_string()];

        let networks =
            compute_master_authorized_networks(&advanced_settings, Some(&qovery_allowed_public_access_cidrs));

        assert_eq!(
            networks,
            vec![
                MasterAuthorizedNetwork {
                    cidr_block: "198.51.100.5/32".to_string(),
                    display_name: "198.51.100.5/32".to_string(),
                },
                MasterAuthorizedNetwork {
                    cidr_block: "203.0.113.10/32".to_string(),
                    display_name: "203.0.113.10/32".to_string(),
                }
            ]
        );
    }
}
