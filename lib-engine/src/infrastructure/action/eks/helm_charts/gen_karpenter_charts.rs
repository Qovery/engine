use crate::environment::models::ToCloudProviderFormat;
use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmChartError};
use crate::infrastructure::action::eks::helm_charts::EksChartsConfigPrerequisites;
use crate::infrastructure::action::eks::helm_charts::karpenter::KarpenterChart;
use crate::infrastructure::action::eks::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use crate::infrastructure::action::eks::helm_charts::karpenter_crd::KarpenterCrdChart;
use crate::infrastructure::helm_charts::ToCommonHelmChart;

pub struct KarpenterCharts {
    pub karpenter_chart: CommonChart,
    pub karpenter_crd_chart: CommonChart,
    pub karpenter_configuration_chart: CommonChart,
}

pub fn generate_karpenter_charts(
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &EksChartsConfigPrerequisites,
) -> Result<KarpenterCharts, CommandError> {
    let karpenter_parameters = chart_config_prerequisites.karpenter_parameters.clone().ok_or_else(|| {
        CommandError::new_from_safe_message(
            "Karpenter parameters should be present when generating karpenter charts".to_string(),
        )
    })?;

    let karpenter_chart_prepare = |metrics_enabled: bool| -> Result<CommonChart, HelmChartError> {
        KarpenterChart::new(
            chart_prefix_path,
            chart_config_prerequisites.cluster_name.to_string(),
            chart_config_prerequisites.karpenter_controller_aws_role_arn.clone(),
            chart_config_prerequisites.is_karpenter_enabled,
            metrics_enabled,
            chart_config_prerequisites.kubernetes_version_upgrade_requested,
        )
        .to_common_helm_chart()
    };

    // Karpenter
    let karpenter_chart = karpenter_chart_prepare(chart_config_prerequisites.metrics_parameters.is_some())?;

    // Karpenter CRD
    let karpenter_crd_chart = KarpenterCrdChart::new(chart_prefix_path).to_common_helm_chart()?;

    // Karpenter Configuration
    let karpenter_configuration_chart = KarpenterConfigurationChart::new(
        chart_prefix_path,
        chart_config_prerequisites.cluster_name.to_string(),
        chart_config_prerequisites.is_karpenter_enabled,
        chart_config_prerequisites.cluster_security_group_id.clone(),
        &chart_config_prerequisites.cluster_id,
        chart_config_prerequisites.cluster_long_id,
        &chart_config_prerequisites.organization_id,
        chart_config_prerequisites.organization_long_id,
        chart_config_prerequisites.kubernetes_version.clone(),
        chart_config_prerequisites.region.to_cloud_provider_format(),
        karpenter_parameters,
        chart_config_prerequisites.infra_options.user_provided_network.as_ref(),
        chart_config_prerequisites.cluster_advanced_settings.pleco_resources_ttl,
    )
    .to_common_helm_chart()?;

    Ok(KarpenterCharts {
        karpenter_chart,
        karpenter_crd_chart,
        karpenter_configuration_chart,
    })
}
