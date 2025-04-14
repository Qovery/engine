use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::aws::AwsStorageType;
use crate::environment::models::gcp::GcpStorageType;
use crate::environment::models::scaleway::ScwStorageType;
use crate::errors::CommandError;
use crate::helm::{CommonChart, HelmAction, HelmChartNamespaces};
use crate::infrastructure::action::eks::helm_charts::EksChartsConfigPrerequisites;
use crate::infrastructure::action::gke::helm_charts::GkeChartsConfigPrerequisites;
use crate::infrastructure::action::scaleway::helm_charts::KapsuleChartsConfigPrerequisites;
use crate::infrastructure::helm_charts::ToCommonHelmChart;
use crate::infrastructure::helm_charts::kube_prometheus_stack_chart::{
    KubePrometheusStackChart, PrometheusConfiguration,
};
use crate::infrastructure::helm_charts::kube_state_metrics::KubeStateMetricsChart;
use crate::infrastructure::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
use crate::infrastructure::helm_charts::prometheus_operator_crds::PrometheusOperatorCrdsChart;
use crate::infrastructure::helm_charts::thanos::ThanosChart;
use crate::io_models::metrics::{MetricsConfiguration, MetricsParameters};
use crate::io_models::models::CustomerHelmChartsOverride;
use std::sync::Arc;
use url::Url;

pub enum CloudProviderMetricsConfig<'a> {
    Eks(&'a EksChartsConfigPrerequisites),
    Gke(&'a GkeChartsConfigPrerequisites),
    Kapsule(&'a KapsuleChartsConfigPrerequisites),
}

impl CloudProviderMetricsConfig<'_> {
    pub fn prometheus_configuration(&self) -> PrometheusConfiguration {
        match self {
            Self::Eks(cfg) => {
                let region = cfg.region.to_cloud_provider_format();
                PrometheusConfiguration::AwsS3 {
                    region: cfg.region.clone(),
                    bucket_name: cfg.aws_s3_prometheus_bucket_name.to_string(),
                    aws_iam_prometheus_role_arn: cfg.aws_iam_eks_prometheus_role_arn.to_string(),
                    endpoint: format!("s3.{}.amazonaws.com", region),
                }
            }
            Self::Gke(cfg) => PrometheusConfiguration::GcpCloudStorage {
                thanos_service_account_email: cfg.thanos_service_account_email.clone(),
                bucket_name: cfg.prometheus_bucket_name.to_string(),
            },
            Self::Kapsule(cfg) => PrometheusConfiguration::ScalewayObjectStorage {
                bucket_name: cfg.prometheus_storage_config_scaleway_s3.to_string(),
                region: cfg.zone.region().to_string(),
                endpoint: Url::parse(&cfg.endpoint)
                    .ok()
                    .and_then(|url| url.host_str().map(|host| host.to_string()))
                    .unwrap_or_else(|| cfg.endpoint.clone()),
                access_key: cfg.access_key.clone(),
                secret_key: cfg.secret_key.clone(),
            },
        }
    }

    pub fn storage_class(&self) -> String {
        match self {
            Self::Eks(_) => AwsStorageType::GP2.to_k8s_storage_class(),
            Self::Gke(_) => GcpStorageType::Balanced.to_k8s_storage_class(),
            Self::Kapsule(_) => ScwStorageType::SbvSsd.to_k8s_storage_class(),
        }
    }

    pub fn is_karpenter_enabled(&self) -> bool {
        match self {
            Self::Eks(cfg) => cfg.is_karpenter_enabled,
            Self::Gke(_) => false,
            Self::Kapsule(_) => false,
        }
    }

    pub fn metrics_parameters(&self) -> Option<&MetricsParameters> {
        match self {
            Self::Eks(cfg) => cfg.metrics_parameters.as_ref(),
            Self::Gke(cfg) => cfg.metrics_parameters.as_ref(),
            Self::Kapsule(cfg) => cfg.metrics_parameters.as_ref(),
        }
    }
}

#[derive(Default)]
pub struct MetricsCharts {
    pub prometheus_operator_crds_chart: Option<CommonChart>,
    pub kube_prometheus_stack_chart: Option<CommonChart>,
    pub thanos_chart: Option<CommonChart>,
    pub prometheus_adapter_chart: Option<CommonChart>,
    pub kube_state_metrics_chart: Option<CommonChart>,
}

pub fn generate_metrics_charts(
    provider_config: CloudProviderMetricsConfig,
    chart_prefix_path: Option<&str>,
    prometheus_internal_url: &str,
    prometheus_namespace: HelmChartNamespaces,
    get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
) -> Result<MetricsCharts, CommandError> {
    let metrics_configuration = provider_config.metrics_parameters().map(|it| it.config.clone());

    match metrics_configuration {
        Some(MetricsConfiguration::MetricsInstalledByQovery {
            install_prometheus_adapter,
        }) => generate_charts_installed_by_qovery(
            HelmAction::Deploy,
            install_prometheus_adapter,
            chart_prefix_path,
            provider_config,
            prometheus_internal_url,
            prometheus_namespace,
            get_chart_override_fn,
        ),
        None => generate_charts_installed_by_qovery(
            HelmAction::Destroy,
            false, // we force a desinstall for prometheus adapter
            chart_prefix_path,
            provider_config,
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
    provider_config: CloudProviderMetricsConfig,
    prometheus_internal_url: &str,
    prometheus_namespace: HelmChartNamespaces,
    get_chart_override_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
) -> Result<MetricsCharts, CommandError> {
    // TODO (ENG-1986) ATM we can't install prometheus operator crds systematically, as some clients may have already installed some versions on their side
    // Prometheus CRDs
    let prometheus_operator_crds_chart = match helm_action {
        HelmAction::Deploy => {
            Some(PrometheusOperatorCrdsChart::new(chart_prefix_path, prometheus_namespace).to_common_helm_chart()?)
        }
        HelmAction::Destroy => None,
    };

    // Kube Prometheus Stack
    let kube_prometheus_stack_chart = KubePrometheusStackChart::new(
        helm_action.clone(),
        chart_prefix_path,
        provider_config.storage_class(),
        prometheus_internal_url.to_string(),
        prometheus_namespace,
        provider_config.prometheus_configuration(),
        true,
        get_chart_override_fn.clone(),
        false,
        provider_config.is_karpenter_enabled(),
    )
    .to_common_helm_chart()?;

    // Thanos
    let thanos_chart = ThanosChart::new(
        helm_action.clone(),
        chart_prefix_path,
        prometheus_namespace,
        None,
        provider_config.prometheus_configuration(),
        provider_config.storage_class(),
        None,
        None,
        None,
        None,
        provider_config.is_karpenter_enabled(),
    )
    .to_common_helm_chart()?;

    // Kube State Metrics
    let kube_state_metrics_chart = KubeStateMetricsChart::new(
        HelmAction::Destroy, //uninstall all kube_state_metrics_chart, as it is now enabled in the kube-prometheus-stack chart.
        // (TODO QOV-595 it can be removed once the chart has been removed from all the clusters)
        chart_prefix_path,
        HelmChartNamespaces::Prometheus,
        true,
        get_chart_override_fn.clone(),
    )
    .to_common_helm_chart()?;

    // Prometheus Adapter
    let prometheus_adapter_helm_action = match install_prometheus_adapter {
        true => HelmAction::Deploy,
        false => HelmAction::Destroy,
    };
    let prometheus_adapter_chart = PrometheusAdapterChart::new(
        prometheus_adapter_helm_action,
        chart_prefix_path,
        prometheus_internal_url.to_string(),
        prometheus_namespace,
        get_chart_override_fn.clone(),
        true,
        provider_config.is_karpenter_enabled(),
    )
    .to_common_helm_chart()?;

    Ok(MetricsCharts {
        prometheus_operator_crds_chart,
        kube_prometheus_stack_chart: Some(kube_prometheus_stack_chart),
        thanos_chart: Some(thanos_chart),
        prometheus_adapter_chart: Some(prometheus_adapter_chart),
        kube_state_metrics_chart: Some(kube_state_metrics_chart),
    })
}
