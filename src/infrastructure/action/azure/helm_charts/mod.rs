mod gen_charts;

use crate::environment::models::domain::ToHelmString;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::EngineError;
use crate::helm::HelmChart;
use crate::infrastructure::action::azure::AksQoveryTerraformOutput;
use crate::infrastructure::action::azure::helm_charts::gen_charts::aks_helm_charts;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::PrometheusConfiguration;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::azure::AksOptions;
use crate::infrastructure::models::kubernetes::azure::aks::AKS;
use crate::io_models::context::Features;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::metrics::MetricsParameters;
use crate::string::terraform_list_format;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub struct AksChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub cluster_creation_date: DateTime<Utc>,
    pub ff_log_history_enabled: bool,
    pub _ff_metrics_history_enabled: bool,
    pub managed_dns_helm_format: String,
    pub _managed_dns_resolvers_terraform_format: String,
    pub managed_dns_root_domain_helm_format: String,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub dns_provider_config: DnsProviderConfiguration,
    pub storage_logging_service_account_name: String,
    pub _storage_logging_service_account_primary_access_key: String,
    pub storage_logging_service_msi_client_id: String,
    pub logs_bucket_name: String,
    pub metrics_parameters: Option<MetricsParameters>,
    pub _prometheus_config: Option<PrometheusConfiguration>,
    // qovery options form json input
    pub _infra_options: AksOptions,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
}

impl AksChartsConfigPrerequisites {
    pub fn new(
        organization_id: String,
        organization_long_id: uuid::Uuid,
        cluster_id: String,
        cluster_long_id: uuid::Uuid,
        cluster_creation_date: DateTime<Utc>,
        ff_log_history_enabled: bool,
        ff_metrics_history_enabled: bool,
        managed_dns_helm_format: String,
        managed_dns_resolvers_terraform_format: String,
        managed_dns_root_domain_helm_format: String,
        lets_encrypt_config: LetsEncryptConfig,
        dns_provider_config: DnsProviderConfiguration,
        storage_logging_service_account_name: String,
        storage_logging_service_account_primary_access_key: String,
        storage_logging_service_msi_client_id: String,
        logs_bucket_name: String,
        metrics_parameters: Option<MetricsParameters>,
        prometheus_config: Option<PrometheusConfiguration>,
        infra_options: AksOptions,
        cluster_advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    ) -> Self {
        Self {
            organization_id,
            organization_long_id,
            cluster_id,
            cluster_long_id,
            cluster_creation_date,
            ff_log_history_enabled,
            _ff_metrics_history_enabled: ff_metrics_history_enabled,
            managed_dns_helm_format,
            _managed_dns_resolvers_terraform_format: managed_dns_resolvers_terraform_format,
            managed_dns_root_domain_helm_format,
            lets_encrypt_config,
            dns_provider_config,
            storage_logging_service_account_name,
            _storage_logging_service_account_primary_access_key: storage_logging_service_account_primary_access_key,
            storage_logging_service_msi_client_id,
            logs_bucket_name,
            metrics_parameters,
            _prometheus_config: prometheus_config,
            _infra_options: infra_options,
            cluster_advanced_settings,
            customer_helm_charts_override,
        }
    }
}

pub struct AksHelmsDeployment<'a> {
    context: HelmInfraContext,
    terraform_output: AksQoveryTerraformOutput,
    cluster: &'a AKS,
}

impl<'a> AksHelmsDeployment<'a> {
    pub fn new(context: HelmInfraContext, terraform_output: AksQoveryTerraformOutput, cluster: &'a AKS) -> Self {
        Self {
            context,
            terraform_output,
            cluster,
        }
    }
}

impl HelmInfraResources for AksHelmsDeployment<'_> {
    type ChartPrerequisite = AksChartsConfigPrerequisites;

    fn charts_context(&self) -> &HelmInfraContext {
        &self.context
    }

    fn new_chart_prerequisite(&self, infra_ctx: &InfrastructureContext) -> Self::ChartPrerequisite {
        AksChartsConfigPrerequisites::new(
            infra_ctx.context().organization_short_id().to_string(),
            *infra_ctx.context().organization_long_id(),
            self.cluster.short_id().to_string(),
            self.cluster.long_id,
            self.cluster.created_at,
            self.cluster.context.is_feature_enabled(&Features::LogsHistory),
            self.cluster.context.is_feature_enabled(&Features::MetricsHistory),
            infra_ctx.dns_provider().domain().to_helm_format_string(),
            terraform_list_format(
                infra_ctx
                    .dns_provider()
                    .resolvers()
                    .iter()
                    .map(|x| x.clone().to_string())
                    .collect(),
            ),
            infra_ctx.dns_provider().domain().root_domain().to_helm_format_string(),
            LetsEncryptConfig::new(
                self.cluster.options.tls_email_report.to_string(),
                self.cluster.context.is_test_cluster(),
            ),
            infra_ctx.dns_provider().provider_configuration(),
            self.terraform_output.main_storage_account_name.clone(),
            self.terraform_output.main_storage_account_primary_access_key.clone(),
            self.terraform_output.loki_logging_service_msi_client_id.clone(),
            self.cluster.logs_bucket_name(),
            self.cluster.options.metrics_parameters.clone(),
            None,
            self.cluster.options.clone(),
            self.cluster.advanced_settings().clone(),
            self.cluster.customer_helm_charts_override.clone(),
        )
    }

    fn gen_charts_to_deploy(
        &self,
        infra_ctx: &InfrastructureContext,
        charts_prerequisites: Self::ChartPrerequisite,
    ) -> Result<Vec<Vec<Box<dyn HelmChart>>>, Box<EngineError>> {
        aks_helm_charts(
            &charts_prerequisites,
            Some(self.context.destination_folder.to_string_lossy().as_ref()),
            &*infra_ctx.context().qovery_api,
            infra_ctx.dns_provider().domain(),
        )
        .map_err(|e| Box::new(EngineError::new_helm_charts_setup_error(self.context.event_details.clone(), e)))
    }
}
