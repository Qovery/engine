use chrono::{DateTime, Utc};

use crate::environment::models::domain::ToHelmString;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::EngineError;
use crate::helm::HelmChart;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::gke::GkeQoveryTerraformOutput;
use crate::infrastructure::action::gke::helm_charts::gen_charts::gke_helm_charts;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::gcp::{Gke, GkeOptions};
use crate::infrastructure::models::kubernetes::keda::{KedaAvailability, KedaResourceProfile};
use crate::io_models::context::Features;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::metrics::MetricsParameters;
use std::collections::HashMap;

pub mod gen_charts;
pub mod gen_eso_charts;
pub mod gen_keda_charts;

pub struct GkeChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub cluster_name: String,
    pub cluster_creation_date: DateTime<Utc>,
    pub ff_log_history_enabled: bool,
    pub managed_dns_root_domain_helm_format: String,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub dns_provider_config: DnsProviderConfiguration,
    pub loki_logging_service_account_email: String,
    pub logs_bucket_name: String,
    pub metrics_parameters: Option<MetricsParameters>,
    // qovery options form json input
    pub infra_options: GkeOptions,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,

    pub thanos_service_account_email: String,
    pub prometheus_bucket_name: String,
    pub is_keda_enabled: bool,
    pub keda_resource_profile: KedaResourceProfile,
    pub keda_availability: KedaAvailability,
    pub gcp_keda_operator_service_account_email: Option<String>,
    pub gcp_keda_metrics_server_service_account_email: Option<String>,
    pub gcp_external_secrets_operator_service_account_email: Option<String>,
}

impl GkeChartsConfigPrerequisites {
    pub fn new(
        organization_id: String,
        organization_long_id: uuid::Uuid,
        cluster_id: String,
        cluster_long_id: uuid::Uuid,
        cluster_name: String,
        cluster_creation_date: DateTime<Utc>,
        ff_log_history_enabled: bool,
        managed_dns_root_domain_helm_format: String,
        lets_encrypt_config: LetsEncryptConfig,
        dns_provider_config: DnsProviderConfiguration,
        loki_logging_service_account_email: String,
        logs_bucket_name: String,
        metrics_parameters: Option<MetricsParameters>,
        infra_options: GkeOptions,
        cluster_advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
        thanos_service_account_email: String,
        prometheus_bucket_name: String,
        is_keda_enabled: bool,
        keda_resource_profile: KedaResourceProfile,
        keda_availability: KedaAvailability,
        gcp_keda_operator_service_account_email: Option<String>,
        gcp_keda_metrics_server_service_account_email: Option<String>,
        gcp_external_secrets_operator_service_account_email: Option<String>,
    ) -> Self {
        Self {
            organization_id,
            organization_long_id,
            cluster_id,
            cluster_long_id,
            cluster_name,
            cluster_creation_date,
            ff_log_history_enabled,
            managed_dns_root_domain_helm_format,
            lets_encrypt_config,
            dns_provider_config,
            loki_logging_service_account_email,
            logs_bucket_name,
            metrics_parameters,
            infra_options,
            cluster_advanced_settings,
            customer_helm_charts_override,
            thanos_service_account_email,
            prometheus_bucket_name,
            is_keda_enabled,
            keda_resource_profile,
            keda_availability,
            gcp_keda_operator_service_account_email,
            gcp_keda_metrics_server_service_account_email,
            gcp_external_secrets_operator_service_account_email,
        }
    }
}

pub struct GkeHelmsDeployment<'a> {
    context: HelmInfraContext,
    terraform_output: GkeQoveryTerraformOutput,
    cluster: &'a Gke,
}

impl<'a> GkeHelmsDeployment<'a> {
    pub fn new(context: HelmInfraContext, terraform_output: GkeQoveryTerraformOutput, cluster: &'a Gke) -> Self {
        Self {
            context,
            terraform_output,
            cluster,
        }
    }
}

impl HelmInfraResources for GkeHelmsDeployment<'_> {
    type ChartPrerequisite = GkeChartsConfigPrerequisites;

    fn charts_context(&self) -> &HelmInfraContext {
        &self.context
    }

    fn new_chart_prerequisite(&self, infra_ctx: &InfrastructureContext) -> Self::ChartPrerequisite {
        let keda_params = self.cluster.get_keda_parameters();

        GkeChartsConfigPrerequisites::new(
            infra_ctx.context().organization_short_id().to_string(),
            *infra_ctx.context().organization_long_id(),
            self.cluster.short_id().to_string(),
            self.cluster.long_id,
            self.cluster.name.to_string(),
            self.cluster.created_at,
            self.cluster.context.is_feature_enabled(&Features::LogsHistory),
            infra_ctx.dns_provider().domain().root_domain().to_helm_format_string(),
            LetsEncryptConfig::new(
                self.cluster.options.tls_email_report.to_string(),
                self.cluster.context.is_test_cluster(),
            ),
            infra_ctx.dns_provider().provider_configuration(),
            self.terraform_output.loki_logging_service_account_email.clone(),
            self.cluster.logs_bucket_name(),
            self.cluster.options.metrics_parameters.clone(),
            self.cluster.options.clone(),
            self.cluster.advanced_settings().clone(),
            self.cluster.customer_helm_charts_override.clone(),
            self.terraform_output.thanos_service_account_email.clone(),
            self.cluster.prometheus_bucket_name(),
            keda_params.as_ref().map(|p| p.enabled).unwrap_or(false),
            keda_params.as_ref().map(|p| p.resource_profile).unwrap_or_default(),
            keda_params.as_ref().map(|p| p.availability).unwrap_or_default(),
            self.terraform_output.keda_operator_service_account_email.clone(),
            self.terraform_output.keda_metrics_server_service_account_email.clone(),
            self.terraform_output
                .external_secrets_operator_service_account_email
                .clone(),
        )
    }

    fn gen_charts_to_deploy(
        &self,
        infra_ctx: &InfrastructureContext,
        charts_prerequisites: Self::ChartPrerequisite,
    ) -> Result<Vec<Vec<Box<dyn HelmChart>>>, Box<EngineError>> {
        gke_helm_charts(
            &charts_prerequisites,
            Some(self.context.destination_folder.to_string_lossy().as_ref()),
            &*infra_ctx.context().qovery_api,
            infra_ctx.dns_provider().domain(),
        )
        .map_err(|e| Box::new(EngineError::new_helm_charts_setup_error(self.context.event_details.clone(), e)))
    }
}
