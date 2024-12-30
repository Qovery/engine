use crate::environment::models::domain::ToTerraformString;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::ToInfraTeraContext;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::Kapsule;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::context::Features;
use crate::string::terraform_list_format;
use reqwest::header;
use serde_derive::{Deserialize, Serialize};
use tera::Context as TeraContext;

impl ToInfraTeraContext for Kapsule {
    fn to_infra_tera_context(&self, infra_ctx: &InfrastructureContext) -> Result<TeraContext, Box<EngineError>> {
        kapsule_tera_context(self, infra_ctx)
    }
}

fn kapsule_tera_context(cluster: &Kapsule, infra_ctx: &InfrastructureContext) -> Result<TeraContext, Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
    let mut context = TeraContext::new();

    // Scaleway
    context.insert("scaleway_project_id", cluster.options.scaleway_project_id.as_str());
    context.insert("scaleway_access_key", cluster.options.scaleway_access_key.as_str());
    context.insert("scaleway_secret_key", cluster.options.scaleway_secret_key.as_str());
    context.insert("scw_region", &cluster.zone.region().as_str());
    context.insert("scw_zone", &cluster.zone.as_str());

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
    context.insert(
        "wildcard_managed_dns",
        &infra_ctx.dns_provider().domain().wildcarded().to_string(),
    );

    // add specific DNS fields
    infra_ctx.dns_provider().insert_into_teracontext(&mut context);

    context.insert("dns_email_report", &cluster.options.tls_email_report);

    // Kubernetes
    context.insert("test_cluster", &cluster.context().is_test_cluster());
    context.insert("kubernetes_cluster_long_id", &cluster.long_id);
    context.insert("kubernetes_cluster_id", cluster.short_id());
    context.insert("kubernetes_cluster_name", cluster.cluster_name().as_str());
    context.insert("kubernetes_cluster_version", &cluster.version.to_string());
    context.insert(
        "kubernetes_cluster_type",
        &cluster.options.scaleway_kubernetes_type.to_terraform_format_string(),
    );

    // Qovery
    context.insert("organization_id", infra_ctx.cloud_provider().organization_id());
    context.insert(
        "organization_long_id",
        &infra_ctx.cloud_provider().organization_long_id().to_string(),
    );
    context.insert("object_storage_kubeconfig_bucket", &cluster.kubeconfig_bucket_name());
    context.insert("object_storage_logs_bucket", &cluster.logs_bucket_name());

    context.insert("qovery_api_url", cluster.options.qovery_api_url.as_str());

    // Qovery features
    context.insert(
        "log_history_enabled",
        &cluster.context().is_feature_enabled(&Features::LogsHistory),
    );
    context.insert(
        "metrics_history_enabled",
        &cluster.context().is_feature_enabled(&Features::MetricsHistory),
    );

    // AWS S3 tfstates storage tfstates
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

    // TLS
    context.insert(
        "acme_server_url",
        LetsEncryptConfig::new(
            cluster.options.tls_email_report.to_string(),
            cluster.context().is_test_cluster(),
        )
        .acme_url()
        .as_str(),
    );

    // grafana credentials
    context.insert("grafana_admin_user", cluster.options.grafana_admin_user.as_str());
    context.insert("grafana_admin_password", cluster.options.grafana_admin_password.as_str());

    // Kubernetes workers
    context.insert("scw_ks_worker_nodes", &cluster.nodes_groups);
    context.insert("scw_ks_pool_autoscale", &true);

    // Advanced settings
    context.insert("load_balancer_size", &cluster.advanced_settings().load_balancer_size);
    context.insert(
        "resource_expiration_in_seconds",
        &cluster.advanced_settings().pleco_resources_ttl,
    );

    // Needed to resolve https://qovery.atlassian.net/browse/ENG-1621
    // Scaleway added a new constraint on scaleway_k8s_cluster to be linked to a private network
    // For existing clusters, exerything is OK
    // For new clusters, we need to inject a resource scaleway_vpc_private_network
    let mut create_private_network = if cluster.context().is_first_cluster_deployment() {
        true
    } else {
        let mut headers = header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert("X-Auth-Token", cluster.options.scaleway_secret_key.parse().unwrap());
        let http = reqwest::blocking::Client::new();
        let tag = format!("ClusterLongId={}", cluster.long_id);
        let url = format!(
            "https://api.scaleway.com/vpc/v2/regions/{}/private-networks?tags={}",
            cluster.region(),
            tag
        );
        match http.get(url).headers(headers.clone()).send() {
            Ok(it) => {
                let result: Result<PrivateNetworksDto, reqwest::Error> = it.json();
                match result {
                    Ok(response) => {
                        // if at least one private network is found, it's OK as we filter with tags
                        !response.private_networks.is_empty()
                    }
                    Err(err) => {
                        return Err(Box::new(EngineError::new_scaleway_cannot_fetch_private_networks(
                            event_details,
                            err.to_string(),
                        )));
                    }
                }
            }
            Err(err) => {
                return Err(Box::new(EngineError::new_scaleway_cannot_fetch_private_networks(
                    event_details,
                    err.to_string(),
                )));
            }
        }
    };
    if cluster.advanced_settings().scaleway_enable_private_network_migration {
        create_private_network = true;
    }
    context.insert("create_private_network", &create_private_network);

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

    Ok(context)
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
struct PrivateNetworksDto {
    private_networks: Vec<PrivateNetworkDto>,
    total_count: u32,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
struct PrivateNetworkDto {
    project_id: String,
}
