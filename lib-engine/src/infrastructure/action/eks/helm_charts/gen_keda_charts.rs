use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction};
use crate::infrastructure::action::eks::helm_charts::EksChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::keda::KedaChart;
use crate::infrastructure::helm_charts::keda_crd::KedaCrdChart;

pub struct KedaCharts {
    pub keda_chart: CommonChart,
    pub keda_crd_chart: CommonChart,
}

pub fn generate_keda_charts(
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &EksChartsConfigPrerequisites,
    is_keda_enabled: bool,
) -> Result<KedaCharts, CommandError> {
    let action = match is_keda_enabled {
        true => HelmAction::Deploy,
        false => HelmAction::Destroy,
    };

    // KEDA CRD Chart
    let keda_crd_chart = KedaCrdChart::new(chart_prefix_path, action.clone()).to_common_helm_chart()?;

    // KEDA Main Chart
    let keda_chart = KedaChart::new(
        chart_prefix_path,
        chart_config_prerequisites.metrics_parameters.is_some(),
        action,
        chart_config_prerequisites.aws_iam_keda_operator_role_arn.clone(),
        chart_config_prerequisites.aws_iam_keda_metrics_server_role_arn.clone(),
    )
    .to_common_helm_chart()?;

    Ok(KedaCharts {
        keda_chart,
        keda_crd_chart,
    })
}
