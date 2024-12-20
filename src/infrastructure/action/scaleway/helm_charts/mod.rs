mod gen_charts;

use crate::environment::models::domain::ToHelmString;
use crate::environment::models::scaleway::ScwZone;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::EngineError;
use crate::helm::HelmChart;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::scaleway::ScalewayQoveryTerraformOutput;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::{Kapsule, KapsuleOptions};
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::context::Features;
use crate::io_models::engine_location::EngineLocation;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::string::terraform_list_format;
use gen_charts::kapsule_helm_charts;
use std::collections::HashMap;

pub struct KapsuleChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub zone: ScwZone,
    pub qovery_engine_location: EngineLocation,
    pub ff_log_history_enabled: bool,
    pub ff_metrics_history_enabled: bool,
    pub ff_grafana_enabled: bool,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub managed_dns_root_domain_helm_format: String,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub dns_provider_config: DnsProviderConfiguration,
    // qovery options form json input
    pub infra_options: KapsuleOptions,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
    pub loki_storage_config_scaleway_s3: String,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
}

impl KapsuleChartsConfigPrerequisites {
    pub fn new(
        organization_id: String,
        organization_long_id: uuid::Uuid,
        cluster_id: String,
        cluster_long_id: uuid::Uuid,
        zone: ScwZone,
        qovery_engine_location: EngineLocation,
        ff_log_history_enabled: bool,
        ff_metrics_history_enabled: bool,
        ff_grafana_enabled: bool,
        managed_dns_helm_format: String,
        managed_dns_resolvers_terraform_format: String,
        managed_dns_root_domain_helm_format: String,
        lets_encrypt_config: LetsEncryptConfig,
        dns_provider_config: DnsProviderConfiguration,
        infra_options: KapsuleOptions,
        cluster_advanced_settings: ClusterAdvancedSettings,
        loki_storage_config_scaleway_s3: String,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    ) -> Self {
        KapsuleChartsConfigPrerequisites {
            organization_id,
            organization_long_id,
            cluster_id,
            cluster_long_id,
            zone,
            qovery_engine_location,
            ff_log_history_enabled,
            ff_metrics_history_enabled,
            ff_grafana_enabled,
            managed_dns_helm_format,
            managed_dns_resolvers_terraform_format,
            managed_dns_root_domain_helm_format,
            lets_encrypt_config,
            dns_provider_config,
            infra_options,
            cluster_advanced_settings,
            loki_storage_config_scaleway_s3,
            customer_helm_charts_override,
        }
    }
}

pub struct KapsuleHelmsDeployment<'a> {
    context: HelmInfraContext,
    terraform_output: ScalewayQoveryTerraformOutput,
    cluster: &'a Kapsule,
}

impl<'a> KapsuleHelmsDeployment<'a> {
    pub fn new(
        context: HelmInfraContext,
        terraform_output: ScalewayQoveryTerraformOutput,
        cluster: &'a Kapsule,
    ) -> Self {
        Self {
            context,
            terraform_output,
            cluster,
        }
    }
}

impl<'a> HelmInfraResources for KapsuleHelmsDeployment<'a> {
    type ChartPrerequisite = KapsuleChartsConfigPrerequisites;

    fn charts_context(&self) -> &HelmInfraContext {
        &self.context
    }

    fn new_chart_prerequisite(&self, infra_ctx: &InfrastructureContext) -> Self::ChartPrerequisite {
        KapsuleChartsConfigPrerequisites::new(
            infra_ctx.cloud_provider().organization_id().to_string(),
            infra_ctx.cloud_provider().organization_long_id(),
            self.cluster.short_id().to_string(),
            self.cluster.long_id,
            self.cluster.zone,
            self.cluster.options.qovery_engine_location.clone(),
            self.cluster.context().is_feature_enabled(&Features::LogsHistory),
            self.cluster.context().is_feature_enabled(&Features::MetricsHistory),
            self.cluster.context().is_feature_enabled(&Features::Grafana),
            infra_ctx.dns_provider().domain().to_helm_format_string(),
            terraform_list_format(
                infra_ctx
                    .dns_provider()
                    .resolvers()
                    .iter()
                    .map(|x| x.to_string())
                    .collect(),
            ),
            infra_ctx.dns_provider().domain().root_domain().to_helm_format_string(),
            LetsEncryptConfig::new(
                self.cluster.options.tls_email_report.to_string(),
                self.cluster.context().is_test_cluster(),
            ),
            infra_ctx.dns_provider().provider_configuration(),
            self.cluster.options.clone(),
            self.cluster.advanced_settings().clone(),
            self.terraform_output.loki_storage_config_scaleway_s3.clone(),
            self.cluster.customer_helm_charts_override.clone(),
        )
    }

    fn gen_charts_to_deploy(
        &self,
        infra_ctx: &InfrastructureContext,
        charts_prerequisites: Self::ChartPrerequisite,
    ) -> Result<Vec<Vec<Box<dyn HelmChart>>>, Box<EngineError>> {
        kapsule_helm_charts(
            &charts_prerequisites,
            Some(self.context.destination_folder.to_string_lossy().as_ref()),
            &*infra_ctx.context().qovery_api,
            infra_ctx.dns_provider().domain(),
        )
        .map_err(|e| Box::new(EngineError::new_helm_charts_setup_error(self.context.event_details.clone(), e)))
    }
}
