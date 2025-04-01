use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::aws::AwsStorageType;
use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction, HelmChartNamespaces};
use crate::infrastructure::action::eks::helm_charts::EksChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::{
    KubePrometheusStackChart, PrometheusConfiguration,
};
use crate::infrastructure::helm_charts::kube_state_metrics::KubeStateMetricsChart;
use crate::infrastructure::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
use crate::infrastructure::helm_charts::prometheus_operator_crds::PrometheusOperatorCrdsChart;
use crate::infrastructure::helm_charts::thanos::ThanosChart;
use crate::io_models::metrics::MetricsConfiguration;
use crate::io_models::models::CustomerHelmChartsOverride;
use std::sync::Arc;

pub struct MetricsCharts {
    pub prometheus_operator_crds_chart: Option<CommonChart>,
    pub kube_prometheus_stack_chart: Option<CommonChart>,
    pub thanos_chart: Option<CommonChart>,
    pub prometheus_adapter_chart: Option<CommonChart>,
    pub kube_state_metrics_chart: Option<CommonChart>,
}

pub fn generate_metrics_charts(
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &EksChartsConfigPrerequisites,
    prometheus_internal_url: &str,
    prometheus_namespace: HelmChartNamespaces,
    get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
) -> Result<MetricsCharts, CommandError> {
    let metrics_configuration = chart_config_prerequisites
        .metrics_parameters
        .as_ref()
        .map(|it| it.config.clone());

    match metrics_configuration {
        Some(MetricsConfiguration::MetricsInstalledByQovery {
            install_prometheus_adapter,
        }) => generate_charts_installed_by_qovery(
            HelmAction::Deploy,
            install_prometheus_adapter,
            chart_prefix_path,
            chart_config_prerequisites,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
        ),
        None => generate_charts_installed_by_qovery(
            HelmAction::Destroy,
            false, // we force a desinstall for prometheus adapter
            chart_prefix_path,
            chart_config_prerequisites,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
        ),
        Some(_) => Ok(MetricsCharts {
            prometheus_operator_crds_chart: None,
            kube_prometheus_stack_chart: None,
            thanos_chart: None,
            prometheus_adapter_chart: None,
            kube_state_metrics_chart: None,
        }),
    }
}

fn generate_charts_installed_by_qovery(
    helm_action: HelmAction,
    install_prometheus_adapter: bool,
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &EksChartsConfigPrerequisites,
    prometheus_internal_url: &str,
    prometheus_namespace: HelmChartNamespaces,
    get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
) -> Result<MetricsCharts, CommandError> {
    let region = chart_config_prerequisites.region.clone();
    let bucket_name = chart_config_prerequisites.aws_s3_prometheus_bucket_name.to_string();
    let aws_iam_prometheus_role_arn = chart_config_prerequisites.aws_iam_eks_prometheus_role_arn.to_string();
    let endpoint = format!("s3.{}.amazonaws.com", region.to_cloud_provider_format());

    // TODO (ENG-1986) ATM we can't install prometheus operator crds systematically, as some clients may have already installed some versions on their side
    // Prometheus CRDs
    let prometheus_operator_crds_chart = match helm_action {
        HelmAction::Deploy => Some(PrometheusOperatorCrdsChart::new(chart_prefix_path).to_common_helm_chart()?),
        HelmAction::Destroy => None,
    };

    // Kube Prometheus Stack
    let kube_prometheus_stack_chart = Some(
        KubePrometheusStackChart::new(
            helm_action.clone(),
            chart_prefix_path,
            AwsStorageType::GP2.to_k8s_storage_class(),
            prometheus_internal_url.to_string(),
            prometheus_namespace,
            PrometheusConfiguration::AwsS3 {
                region: region.clone(),
                bucket_name: bucket_name.clone(),
                aws_iam_prometheus_role_arn: aws_iam_prometheus_role_arn.clone(),
                endpoint: endpoint.clone(),
            },
            true,
            get_chart_override_fn.clone(),
            false,
            chart_config_prerequisites.is_karpenter_enabled,
        )
        .to_common_helm_chart()?,
    );

    // Thanos
    let thanos_chart = Some(
        ThanosChart::new(
            helm_action.clone(),
            chart_prefix_path,
            prometheus_namespace,
            None,
            PrometheusConfiguration::AwsS3 {
                region,
                bucket_name,
                aws_iam_prometheus_role_arn,
                endpoint,
            },
            AwsStorageType::GP2.to_k8s_storage_class(),
            None,
            None,
            None,
            None,
            chart_config_prerequisites.is_karpenter_enabled,
        )
        .to_common_helm_chart()?,
    );

    // Kube State Metrics
    let kube_state_metrics_chart = Some(
        KubeStateMetricsChart::new(
            helm_action,
            chart_prefix_path,
            HelmChartNamespaces::Prometheus,
            true,
            get_chart_override_fn.clone(),
        )
        .to_common_helm_chart()?,
    );

    // Prometheus Adapter
    let prometheus_adapter_helm_action = match install_prometheus_adapter {
        true => HelmAction::Deploy,
        false => HelmAction::Destroy,
    };
    let prometheus_adapter_chart = Some(
        PrometheusAdapterChart::new(
            prometheus_adapter_helm_action,
            chart_prefix_path,
            prometheus_internal_url.to_string(),
            prometheus_namespace,
            get_chart_override_fn.clone(),
            true,
            chart_config_prerequisites.is_karpenter_enabled,
        )
        .to_common_helm_chart()?,
    );

    Ok(MetricsCharts {
        prometheus_operator_crds_chart,
        kube_prometheus_stack_chart,
        thanos_chart,
        prometheus_adapter_chart,
        kube_state_metrics_chart,
    })
}
