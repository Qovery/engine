use crate::environment::models::domain::ToHelmString;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::EngineError;
use crate::helm::HelmChart;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::eksanywhere::helm_charts::gen_charts::eks_anywhere_helm_charts;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::eksanywhere::{EksAnywhere, EksAnywhereOptions};
use crate::io_models::context::Features;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::metrics::MetricsParameters;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

mod gen_charts;
pub mod metal_lb_chart;
pub mod metal_lb_config_chart;

#[derive(Clone)]
pub struct EksAnywhereChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub cluster_creation_date: DateTime<Utc>,
    pub ff_log_history_enabled: bool,
    pub managed_dns_helm_format: String,
    pub managed_dns_root_domain_helm_format: String,
    pub dns_provider_config: DnsProviderConfiguration,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub infra_options: EksAnywhereOptions,
    pub metrics_parameters: Option<MetricsParameters>,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
}

impl EksAnywhereChartsConfigPrerequisites {
    pub fn new(
        organization_id: String,
        organization_long_id: uuid::Uuid,
        cluster_id: String,
        cluster_long_id: uuid::Uuid,
        cluster_creation_date: DateTime<Utc>,
        ff_log_history_enabled: bool,
        managed_dns_helm_format: String,
        managed_dns_root_domain_helm_format: String,
        dns_provider_config: DnsProviderConfiguration,
        lets_encrypt_config: LetsEncryptConfig,
        infra_options: EksAnywhereOptions,
        metrics_parameters: Option<MetricsParameters>,
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
            managed_dns_helm_format,
            managed_dns_root_domain_helm_format,
            dns_provider_config,
            lets_encrypt_config,
            infra_options,
            metrics_parameters,
            cluster_advanced_settings,
            customer_helm_charts_override,
        }
    }
}

pub struct EksAnywhereHelmsDeployment<'a> {
    context: HelmInfraContext,
    cluster: &'a EksAnywhere,
}

impl<'a> EksAnywhereHelmsDeployment<'a> {
    pub fn new(context: HelmInfraContext, cluster: &'a EksAnywhere) -> Self {
        Self { context, cluster }
    }
}

impl HelmInfraResources for EksAnywhereHelmsDeployment<'_> {
    type ChartPrerequisite = EksAnywhereChartsConfigPrerequisites;

    fn charts_context(&self) -> &HelmInfraContext {
        &self.context
    }

    fn new_chart_prerequisite(&self, infra_ctx: &InfrastructureContext) -> Self::ChartPrerequisite {
        EksAnywhereChartsConfigPrerequisites::new(
            infra_ctx.context().organization_short_id().to_string(),
            *infra_ctx.context().organization_long_id(),
            self.cluster.short_id().to_string(),
            self.cluster.long_id,
            self.cluster.created_at,
            self.cluster.context.is_feature_enabled(&Features::LogsHistory),
            infra_ctx.dns_provider().domain().to_helm_format_string(),
            infra_ctx.dns_provider().domain().root_domain().to_helm_format_string(),
            infra_ctx.dns_provider().provider_configuration(),
            LetsEncryptConfig::new(
                self.cluster.options.tls_email_report.to_string(),
                self.cluster.context.is_test_cluster(),
            ),
            self.cluster.options.clone(),
            self.cluster.options.metrics_parameters.clone(),
            self.cluster.advanced_settings().clone(),
            self.cluster.customer_helm_charts_override.clone(),
        )
    }

    fn gen_charts_to_deploy(
        &self,
        infra_ctx: &InfrastructureContext,
        charts_prerequisites: Self::ChartPrerequisite,
    ) -> Result<Vec<Vec<Box<dyn HelmChart>>>, Box<EngineError>> {
        eks_anywhere_helm_charts(
            &charts_prerequisites,
            Some(self.context.destination_folder.to_string_lossy().as_ref()),
            &*infra_ctx.context().qovery_api,
            infra_ctx.dns_provider().domain(),
        )
        .map_err(|e| Box::new(EngineError::new_helm_charts_setup_error(self.context.event_details.clone(), e)))
    }
}
