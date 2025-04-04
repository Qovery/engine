use crate::environment::models::scaleway::ScwStorageType;
use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction, HelmChartNamespaces};
use crate::infrastructure::action::scaleway::helm_charts::KapsuleChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::{
    KubePrometheusStackChart, PrometheusConfiguration,
};
use crate::infrastructure::helm_charts::kube_state_metrics::KubeStateMetricsChart;
use crate::infrastructure::helm_charts::prometheus_operator_crds::PrometheusOperatorCrdsChart;
use crate::infrastructure::helm_charts::thanos::ThanosChart;
use crate::io_models::metrics::MetricsConfiguration;
use crate::io_models::models::CustomerHelmChartsOverride;
use std::sync::Arc;
use url::Url;

pub struct MetricsCharts {
    pub prometheus_operator_crds_chart: Option<CommonChart>,
    pub kube_prometheus_stack_chart: Option<CommonChart>,
    pub thanos_chart: Option<CommonChart>,
    pub kube_state_metrics_chart: Option<CommonChart>,
}

pub fn generate_metrics_charts(
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &KapsuleChartsConfigPrerequisites,
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
            kube_state_metrics_chart: None,
        }),
    }
}

fn generate_charts_installed_by_qovery(
    helm_action: HelmAction,
    _install_prometheus_adapter: bool,
    chart_prefix_path: Option<&str>,
    chart_config_prerequisites: &KapsuleChartsConfigPrerequisites,
    prometheus_internal_url: &str,
    prometheus_namespace: HelmChartNamespaces,
    get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
) -> Result<MetricsCharts, CommandError> {
    let bucket_name = chart_config_prerequisites
        .prometheus_storage_config_scaleway_s3
        .to_string();

    let endpoint = Url::parse(&chart_config_prerequisites.endpoint)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_string()))
        .unwrap_or_else(|| chart_config_prerequisites.endpoint.clone());

    // Prometheus CRDs
    let prometheus_operator_crds_chart = match helm_action {
        HelmAction::Deploy => {
            Some(PrometheusOperatorCrdsChart::new(chart_prefix_path, prometheus_namespace).to_common_helm_chart()?)
        }
        HelmAction::Destroy => None,
    };

    // Kube Prometheus Stack
    let kube_prometheus_stack_chart = Some(
        KubePrometheusStackChart::new(
            helm_action.clone(),
            chart_prefix_path,
            ScwStorageType::SbvSsd.to_k8s_storage_class(),
            prometheus_internal_url.to_string(),
            prometheus_namespace,
            PrometheusConfiguration::ScalewayObjectStorage {
                bucket_name: bucket_name.clone(),
                region: chart_config_prerequisites.zone.region().to_string(),
                endpoint: endpoint.to_string(),
                access_key: chart_config_prerequisites.access_key.clone(),
                secret_key: chart_config_prerequisites.secret_key.clone(),
            },
            true,
            get_chart_override_fn.clone(),
            false,
            false,
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
            PrometheusConfiguration::ScalewayObjectStorage {
                bucket_name: bucket_name.clone(),
                region: chart_config_prerequisites.zone.region().to_string(),
                endpoint: endpoint.to_string(),
                access_key: chart_config_prerequisites.access_key.clone(),
                secret_key: chart_config_prerequisites.secret_key.clone(),
            },
            ScwStorageType::SbvSsd.to_k8s_storage_class(),
            None,
            None,
            None,
            None,
            false,
        )
        .to_common_helm_chart()?,
    );

    // Kube State Metrics
    let kube_state_metrics_chart = Some(
        KubeStateMetricsChart::new(
            helm_action,
            chart_prefix_path,
            prometheus_namespace,
            true,
            get_chart_override_fn.clone(),
        )
        .to_common_helm_chart()?,
    );

    Ok(MetricsCharts {
        prometheus_operator_crds_chart,
        kube_prometheus_stack_chart,
        thanos_chart,
        kube_state_metrics_chart,
    })
}
