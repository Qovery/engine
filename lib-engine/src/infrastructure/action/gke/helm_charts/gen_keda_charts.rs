use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction, HelmChartNamespaces};
use crate::infrastructure::action::gke::helm_charts::GkeChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::keda::{KedaChart, KedaIamConfiguration};
use crate::infrastructure::helm_charts::keda_crd::KedaCrdChart;

pub struct KedaCharts {
    pub keda_chart: CommonChart,
    pub keda_crd_chart: CommonChart,
}

pub fn generate_keda_charts(
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &GkeChartsConfigPrerequisites,
    is_keda_enabled: bool,
) -> Result<KedaCharts, CommandError> {
    let action = match is_keda_enabled {
        true => HelmAction::Deploy,
        false => HelmAction::Destroy,
    };

    // KEDA CRD Chart - For GCP, must be in the qovery namespace
    let mut keda_crd_chart = KedaCrdChart::new(chart_prefix_path, action.clone()).to_common_helm_chart()?;
    keda_crd_chart.chart_info.namespace = HelmChartNamespaces::Qovery;

    let iam_configuration = match (
        &chart_config_prerequisites.gcp_keda_operator_service_account_email,
        &chart_config_prerequisites.gcp_keda_metrics_server_service_account_email,
    ) {
        (Some(operator_sa_email), Some(metrics_server_sa_email)) => Some(KedaIamConfiguration::Gcp {
            operator_service_account_email: operator_sa_email.clone(),
            metrics_server_service_account_email: metrics_server_sa_email.clone(),
        }),
        _ => None,
    };

    let keda_chart = KedaChart::new(
        chart_prefix_path,
        chart_config_prerequisites.metrics_parameters.is_some(),
        action,
        chart_config_prerequisites.keda_resource_profile,
        chart_config_prerequisites.keda_availability,
        iam_configuration,
    )
    .to_common_helm_chart()?;

    Ok(KedaCharts {
        keda_chart,
        keda_crd_chart,
    })
}
