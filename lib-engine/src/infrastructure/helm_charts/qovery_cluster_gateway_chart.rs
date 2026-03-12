use crate::environment::models::domain::Domain;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::load_balancer::{InteractWithLoadBalancer, LoadBalancer};

#[derive(Default)]
pub struct QoveryClusterGatewayChartOptions {
    pub x_forwarded_for_number_truster_hops: Option<u8>, // https://gateway.envoyproxy.io/v1.4/tasks/traffic/client-traffic-policy/#configure-client-ip-detection
    pub custom_http_errors_default: Option<String>, // comma-separated HTTP status codes for gateway-level custom error pages
    pub compression_enable: bool, // enable response compression (brotli quality=6 and gzip level=6, matching nginx defaults)
    pub default_backend_enable: bool, // enable default backend deployment (matches nginx defaultBackend.enabled)
    pub default_backend_image: Option<String>, // default backend container image (e.g., "registry.k8s.io/ingress-nginx/custom-error-pages")
    pub default_backend_tag: Option<String>,   // default backend container image tag (e.g., "v1.1.1")
}

pub struct QoveryClusterGatewayChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    domain: Domain,
    load_balancer: LoadBalancer,
    chart_options: QoveryClusterGatewayChartOptions,
    metrics_enabled: bool,
}

impl QoveryClusterGatewayChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        namespace: HelmChartNamespaces,
        domain: Domain,
        load_balancer: LoadBalancer,
        chart_options: QoveryClusterGatewayChartOptions,
        metrics_enabled: bool,
    ) -> Self {
        QoveryClusterGatewayChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryClusterGatewayChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                QoveryClusterGatewayChart::chart_name(),
            ),
            namespace,
            domain,
            load_balancer,
            chart_options,
            metrics_enabled,
        }
    }

    pub fn chart_name() -> String {
        "qovery-cluster-gateway".to_string()
    }
}

impl ToCommonHelmChart for QoveryClusterGatewayChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut chart_set_values = vec![ChartSetValue {
            key: "dns.domain".to_string(),
            value: self.domain.wildcarded().to_string(),
        }];

        if let Some(num_hops) = self.chart_options.x_forwarded_for_number_truster_hops {
            chart_set_values.push(ChartSetValue {
                key: "gateway.qoveryPublic.xForwardedFor.numberTrustedHops".to_string(),
                value: num_hops.to_string(),
            });
        }

        if let Some(ref custom_http_errors) = self.chart_options.custom_http_errors_default {
            chart_set_values.push(ChartSetValue {
                key: "gateway.qoveryPublic.customHttpErrors.default".to_string(),
                // Escape commas for Helm --set syntax
                value: custom_http_errors.replace(',', "\\,"),
            });
        }

        chart_set_values.push(ChartSetValue {
            key: "gateway.qoveryPublic.compression.enable".to_string(),
            value: self.chart_options.compression_enable.to_string(),
        });

        chart_set_values.push(ChartSetValue {
            key: "gateway.qoveryPublic.defaultBackend.enable".to_string(),
            value: self.chart_options.default_backend_enable.to_string(),
        });

        if let Some(ref image) = self.chart_options.default_backend_image {
            chart_set_values.push(ChartSetValue {
                key: "gateway.qoveryPublic.defaultBackend.image".to_string(),
                value: image.clone(),
            });
        }

        if let Some(ref tag) = self.chart_options.default_backend_tag {
            chart_set_values.push(ChartSetValue {
                key: "gateway.qoveryPublic.defaultBackend.tag".to_string(),
                value: tag.clone(),
            });
        }

        if let Some(annotations) = self.load_balancer.annotations() {
            for (key, value) in annotations {
                chart_set_values.push(ChartSetValue {
                    // Escape dots in annotation keys to prevent Helm from treating them as nested maps
                    key: format!("infrastructure.annotations.{}", key.replace('.', "\\.")),
                    value,
                });
            }
        }

        // enable metrics only if prometheus is installed
        chart_set_values.push(ChartSetValue {
            key: "metrics.enabled".to_string(),
            value: self.metrics_enabled.to_string(),
        });

        chart_set_values.push(ChartSetValue {
            key: "metrics.podMonitor.enabled".to_string(),
            value: self.metrics_enabled.to_string(),
        });

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryClusterGatewayChart::chart_name(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: chart_set_values,
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryClusterGatewayChartInstallationChecker::new())),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct QoveryClusterGatewayChartInstallationChecker {}

impl QoveryClusterGatewayChartInstallationChecker {
    pub fn new() -> Self {
        QoveryClusterGatewayChartInstallationChecker {}
    }
}
impl Default for QoveryClusterGatewayChartInstallationChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartInstallationChecker for QoveryClusterGatewayChartInstallationChecker {
    fn verify_installation(&self, _kube_client: &kube::Client) -> Result<(), CommandError> {
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::models::domain::Domain;
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::qovery_cluster_gateway_chart::{
        QoveryClusterGatewayChart, QoveryClusterGatewayChartOptions,
    };
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind;
    use crate::infrastructure::models::load_balancer::LoadBalancer;
    use crate::infrastructure::models::load_balancer::aws_alb_load_balancer::AwsAlbLoadBalancer;
    use crate::io_models::QoveryIdentifier;
    use std::env;

    fn get_domain() -> Domain {
        Domain::new("qovery.com".to_string())
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_cluster_gateway_chart_directory_exists_test() {
        // setup:
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            LoadBalancer::AwsAlb(AwsAlbLoadBalancer {
                cluster_id: QoveryIdentifier::new_random(),
                organization_id: QoveryIdentifier::new_random(),
            }),
            QoveryClusterGatewayChartOptions::default(),
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            QoveryClusterGatewayChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn qovery_cluster_gateway_chart_values_file_exists_test() {
        // setup:
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            LoadBalancer::AwsAlb(AwsAlbLoadBalancer {
                cluster_id: QoveryIdentifier::new_random(),
                organization_id: QoveryIdentifier::new_random(),
            }),
            QoveryClusterGatewayChartOptions::default(),
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(Kind::Eks),
            ),
            QoveryClusterGatewayChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn qovery_cluster_gateway_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            LoadBalancer::AwsAlb(AwsAlbLoadBalancer {
                cluster_id: QoveryIdentifier::new_random(),
                organization_id: QoveryIdentifier::new_random(),
            }),
            QoveryClusterGatewayChartOptions::default(),
            false,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(Kind::Eks),
                ),
                QoveryClusterGatewayChart::chart_name()
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }
}
