use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction, HelmChartNamespaces};
use crate::infrastructure::action::eks::helm_charts::EksChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::eso_chart::{EsoChart, EsoClusterOutputs};
use crate::infrastructure::helm_charts::eso_config_chart::EsoConfigChart;
use crate::infrastructure::helm_charts::eso_requirements_chart::EsoRequirementsChart;

pub struct EsoCharts {
    pub eso_requirements_chart: CommonChart,
    pub eso_chart: CommonChart,
    pub eso_config_chart: CommonChart,
    pub helm_action: HelmAction,
}

pub fn generate_eso_charts(
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &EksChartsConfigPrerequisites,
) -> Result<EsoCharts, CommandError> {
    let secrets_manager_accesses = chart_config_prerequisites
        .infra_options
        .secrets_manager_accesses()
        .map_err(|e| CommandError::new_from_safe_message(e.to_string()))?;

    let is_eso_enabled = !secrets_manager_accesses.is_empty();

    let helm_action = match is_eso_enabled {
        true => HelmAction::Deploy,
        false => HelmAction::Destroy,
    };

    let secrets_manager_accesses = if is_eso_enabled {
        Some(secrets_manager_accesses)
    } else {
        None
    };
    let role_arn_automatically_generated = chart_config_prerequisites
        .aws_iam_external_secrets_operator_role_arn
        .clone();
    let eso_requirements =
        EsoRequirementsChart::new(chart_prefix_path, helm_action.clone(), HelmChartNamespaces::KubeSystem)
            .to_common_helm_chart()?;
    let eso_cluster_outputs = EsoClusterOutputs::Aws {
        role_arn_automatically_generated: role_arn_automatically_generated.clone(),
    };
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
        helm_action.clone(),
        secrets_manager_accesses,
        eso_cluster_outputs,
    )
    .to_common_helm_chart()?;

    Ok(EsoCharts {
        eso_requirements_chart: eso_requirements,
        eso_chart: eso,
        eso_config_chart: eso_config,
        helm_action,
    })
}
