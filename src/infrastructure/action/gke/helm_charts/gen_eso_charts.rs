use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction, HelmChartNamespaces};
use crate::infrastructure::action::gke::helm_charts::GkeChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::eso_chart::{EsoChart, EsoClusterOutputs};
use crate::infrastructure::helm_charts::eso_config_chart::EsoConfigChart;
use crate::infrastructure::helm_charts::eso_requirements_chart::EsoRequirementsChart;

pub struct EsoCharts {
    pub eso_requirements_chart: CommonChart,
    pub eso_chart: CommonChart,
    pub eso_config_chart: CommonChart,
}

pub fn generate_eso_charts(
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &GkeChartsConfigPrerequisites,
) -> Result<EsoCharts, CommandError> {
    let secrets_manager_accesses = chart_config_prerequisites
        .infra_options
        .secrets_manager_accesses
        .clone();

    let is_eso_enabled = !secrets_manager_accesses.is_empty();

    let (helm_action, secrets_manager_accesses) = match is_eso_enabled {
        true => (HelmAction::Deploy, Some(secrets_manager_accesses)),
        false => (HelmAction::Destroy, None),
    };
    let eso_cluster_outputs = EsoClusterOutputs::Gcp {
        eso_operator_service_account_email: chart_config_prerequisites
            .gcp_external_secrets_operator_service_account_email
            .clone(),
    };

    let eso_requirements =
        EsoRequirementsChart::new(chart_prefix_path, helm_action.clone(), HelmChartNamespaces::Qovery)
            .to_common_helm_chart()?;

    let eso = EsoChart::new(
        chart_prefix_path,
        HelmChartNamespaces::Qovery,
        helm_action.clone(),
        secrets_manager_accesses.clone(),
        eso_cluster_outputs.clone(),
    )
    .to_common_helm_chart()?;

    let eso_config = EsoConfigChart::new(
        chart_prefix_path,
        HelmChartNamespaces::Qovery,
        helm_action,
        secrets_manager_accesses,
        eso_cluster_outputs,
    )
    .to_common_helm_chart()?;

    Ok(EsoCharts {
        eso_requirements_chart: eso_requirements,
        eso_chart: eso,
        eso_config_chart: eso_config,
    })
}
