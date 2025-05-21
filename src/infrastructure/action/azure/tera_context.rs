use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::EngineError;
use crate::helm::HelmChartNamespaces;
use crate::infrastructure::action::ToInfraTeraContext;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::azure::aks::AKS;
use crate::io_models::context::Features;
use crate::string::terraform_list_format;
use tera::Context as TeraContext;

impl ToInfraTeraContext for AKS {
    fn to_infra_tera_context(&self, infra_ctx: &InfrastructureContext) -> Result<TeraContext, Box<EngineError>> {
        aks_tera_context(self, infra_ctx)
    }
}

fn aks_tera_context(cluster: &AKS, infra_ctx: &InfrastructureContext) -> Result<TeraContext, Box<EngineError>> {
    let mut context = TeraContext::new();

    // Qovery
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

    // AZURE
    context.insert("azure_location", cluster.location.to_cloud_provider_format());
    context.insert(
        "azure_zones",
        &cluster
            .location
            .zones()
            .iter()
            .map(|x| x.to_cloud_provider_format())
            .collect::<Vec<_>>(),
    );

    // Node groups
    context.insert("node_group_default", &cluster.node_groups.get_default_node_group());
    context.insert("node_groups_additional", &cluster.node_groups.get_additional_node_groups());

    // Credentials
    context.insert("azure_client_id", cluster.credentials.client_id.as_str());
    context.insert("azure_client_secret", cluster.credentials.client_secret.as_str());
    context.insert("azure_tenant_id", cluster.credentials.tenant_id.as_str());
    context.insert("azure_subscription_id", cluster.credentials.subscription_id.as_str());
    context.insert("azure_resource_group_name", cluster.options.azure_resource_group_name.as_str());

    // Storage
    context.insert("main_storage_account_name", cluster.cluster_name().replace('-', "").as_str()); // can only consist of lowercase letters and numbers, and must be between 3 and 24 characters long

    // Network
    // VPC

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

    // Loki
    context.insert("loki_namespace", HelmChartNamespaces::Qovery.to_string().as_str());

    Ok(context)
}
