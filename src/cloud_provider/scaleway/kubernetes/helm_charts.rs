use crate::cloud_provider::helm::HelmChart;
use crate::cloud_provider::scaleway::kubernetes::Options;
use crate::error::SimpleError;
use std::path::Path;

pub struct ChartsConfigPrerequisites {
    pub organization_id: String,
    pub default_project_id: String,
    pub region: String,
    pub cluster_name: String,
    pub cloud_provider: String,
    pub test_cluster: bool,
    pub scw_access_key: String,
    pub scw_secret_key: String,
    pub ff_log_history_enabled: bool,
    pub ff_metrics_history_enabled: bool,
    pub managed_dns_name: String,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub external_dns_provider: String,
    pub dns_email_report: String,
    pub acme_url: String,
    pub cloudflare_email: String,
    pub cloudflare_api_token: String,
    pub disable_pleco: bool,
    // qovery options form json input
    pub infra_options: Options,
}

impl ChartsConfigPrerequisites {
    pub fn new(
        organization_id: String,
        default_project_id: String,
        region: String,
        cluster_name: String,
        cloud_provider: String,
        test_cluster: bool,
        scw_access_key: String,
        scw_secret_key: String,
        ff_log_history_enabled: bool,
        ff_metrics_history_enabled: bool,
        managed_dns_name: String,
        managed_dns_helm_format: String,
        managed_dns_resolvers_terraform_format: String,
        external_dns_provider: String,
        dns_email_report: String,
        acme_url: String,
        cloudflare_email: String,
        cloudflare_api_token: String,
        disable_pleco: bool,
        infra_options: Options,
    ) -> Self {
        ChartsConfigPrerequisites {
            organization_id,
            default_project_id,
            region,
            cluster_name,
            cloud_provider,
            test_cluster,
            scw_access_key,
            scw_secret_key,
            ff_log_history_enabled,
            ff_metrics_history_enabled,
            managed_dns_name,
            managed_dns_helm_format,
            managed_dns_resolvers_terraform_format,
            external_dns_provider,
            dns_email_report,
            acme_url,
            cloudflare_email,
            cloudflare_api_token,
            disable_pleco,
            infra_options,
        }
    }
}

pub fn scw_helm_charts(
    qovery_terraform_config_file: &str,
    chart_config_prerequisites: &ChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    kubernetes_config: &Path,
    envs: &[(String, String)],
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, SimpleError> {
    info!("preparing chart configuration to be deployed");

    Ok(vec![])
}
