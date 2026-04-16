use crate::environment::models::domain::Domain;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::load_balancer::{InteractWithLoadBalancer, LoadBalancer};
use crate::runtime::block_on;
use crate::services::kube_client::Gateway;
use ipnet::IpNet;
use kube::Api;
use retry::OperationResult;
use retry::delay::Fixed;

#[derive(Default)]
pub enum XForwardedForClientIpDetection {
    #[default]
    None,
    TrustedHops(u8),          // fixed number of trusted proxy hops in XFF
    TrustedCIDRs(Vec<IpNet>), // trusted CIDR ranges for XFF client IP detection
}

impl XForwardedForClientIpDetection {
    /// Build an XFF client IP detection strategy from cluster advanced settings.
    ///
    /// Envoy Gateway ClientTrafficPolicy enforces a one-of constraint:
    /// only one of `numTrustedHops` or `trustedCIDRs` can be configured.
    /// If both settings are provided by input, we intentionally prioritize
    /// `trustedCIDRs` to keep rendered policy valid and deterministic.
    pub fn from_trusted_cidrs_and_hops(trusted_cidrs: &[IpNet], trusted_hops: Option<u8>) -> Self {
        if !trusted_cidrs.is_empty() {
            return XForwardedForClientIpDetection::TrustedCIDRs(trusted_cidrs.to_vec());
        }

        if let Some(num_hops) = trusted_hops {
            return XForwardedForClientIpDetection::TrustedHops(num_hops);
        }

        XForwardedForClientIpDetection::None
    }
}

#[derive(Default)]
pub struct QoveryClusterGatewayChartOptions {
    pub x_forwarded_for_client_ip_detection: XForwardedForClientIpDetection, // https://gateway.envoyproxy.io/v1.4/tasks/traffic/client-traffic-policy/#configure-client-ip-detection
    pub http_stream_idle_timeout_seconds: Option<u32>, // stream idle timeout for downstream HTTP streams
    pub custom_http_errors_default: Option<String>, // comma-separated HTTP status codes for gateway-level custom error pages
    pub compression_enable: bool, // enable response compression (brotli quality=6 and gzip level=6, matching nginx defaults)
    pub default_backend_enable: bool, // enable default backend deployment (matches nginx defaultBackend.enabled)
    pub default_backend_image: Option<String>, // default backend container image (e.g., "registry.k8s.io/ingress-nginx/custom-error-pages")
    pub default_backend_tag: Option<String>,   // default backend container image tag (e.g., "v1.1.1")
    pub reconcile_gateway_cert_refs: bool,     // reconcile Gateway TLS certificateRefs post-install/upgrade
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

        match &self.chart_options.x_forwarded_for_client_ip_detection {
            XForwardedForClientIpDetection::TrustedHops(num_hops) => {
                chart_set_values.push(ChartSetValue {
                    key: "gateway.qoveryPublic.xForwardedFor.numberTrustedHops".to_string(),
                    value: num_hops.to_string(),
                });
            }
            XForwardedForClientIpDetection::TrustedCIDRs(trusted_cidrs) => {
                for (index, trusted_cidr) in trusted_cidrs.iter().enumerate() {
                    chart_set_values.push(ChartSetValue {
                        key: format!("gateway.qoveryPublic.xForwardedFor.trustedCIDRs[{index}]"),
                        value: trusted_cidr.to_string(),
                    });
                }
            }
            XForwardedForClientIpDetection::None => {}
        }

        if let Some(stream_idle_timeout_seconds) = self.chart_options.http_stream_idle_timeout_seconds {
            chart_set_values.push(ChartSetValue {
                key: "gateway.qoveryPublic.timeout.http.streamIdleTimeoutSeconds".to_string(),
                value: stream_idle_timeout_seconds.to_string(),
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
            chart_installation_checker: Some(Box::new(QoveryClusterGatewayChartInstallationChecker::new(
                self.namespace.clone(),
                self.chart_options.reconcile_gateway_cert_refs,
            ))),
            vertical_pod_autoscaler: None,
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
pub struct QoveryClusterGatewayChartInstallationChecker {
    namespace: HelmChartNamespaces,
    reconcile_gateway_cert_refs: bool,
}

impl QoveryClusterGatewayChartInstallationChecker {
    pub fn new(namespace: HelmChartNamespaces, reconcile_gateway_cert_refs: bool) -> Self {
        QoveryClusterGatewayChartInstallationChecker {
            namespace,
            reconcile_gateway_cert_refs,
        }
    }

    fn has_condition_true_for_generation(gateway: &Gateway, conditions_type: &str, expected_generation: i64) -> bool {
        gateway
            .status
            .as_ref()
            .and_then(|status| status.conditions.as_ref())
            .and_then(|conditions| conditions.iter().find(|condition| condition.type_ == conditions_type))
            .map(|condition| {
                condition.status == "True" && condition.observed_generation.unwrap_or_default() >= expected_generation
            })
            .unwrap_or(false)
    }
}
impl Default for QoveryClusterGatewayChartInstallationChecker {
    fn default() -> Self {
        Self::new(HelmChartNamespaces::Qovery, false)
    }
}

impl ChartInstallationChecker for QoveryClusterGatewayChartInstallationChecker {
    fn verify_installation(&self, kube_client: &kube::Client) -> Result<(), CommandError> {
        let gateway_name = "qovery-cluster-public-gateway";
        let namespace = self.namespace.to_string();
        let kube_client = kube_client.clone();

        let result = retry::retry(Fixed::from_millis(5000).take(24), || {
            // Retry every 5 seconds for up to 2 minutes (24 attempts * 5s = 120s)
            let gateways: Api<Gateway> = Api::namespaced(kube_client.clone(), namespace.as_str());

            let gateway = match block_on(gateways.get(gateway_name)) {
                Ok(result) => result,
                Err(e) => {
                    let err = CommandError::new(
                        format!("Error trying to get gateway (name={gateway_name}, namespace={namespace})"),
                        Some(e.to_string()),
                        None,
                    );
                    return OperationResult::Retry(err);
                }
            };

            let expected_generation = gateway.metadata.generation.unwrap_or_default();
            let is_accepted = Self::has_condition_true_for_generation(&gateway, "Accepted", expected_generation);
            let is_programmed = Self::has_condition_true_for_generation(&gateway, "Programmed", expected_generation);

            // Phase 1 check for "qovery-cluster-gateway": ensure the Gateway object exists.
            // The retry loop here is only for transient read failures / resource-not-found while the
            // Gateway object is being created. Once `get()` succeeds, this checker returns Ok(()) even if
            // readiness conditions are not met yet.
            // Phase 2 readiness (Accepted/Programmed) is enforced later in EnvoyGatewayChartChecker,
            // after the Envoy controller is deployed in a subsequent Helm level.
            if !is_accepted || !is_programmed {
                tracing::info!(
                    "Gateway exists but is not yet accepted/programmed (name={gateway_name}, namespace={namespace}, accepted={is_accepted}, programmed={is_programmed}, generation={expected_generation})"
                );
            }

            OperationResult::Ok(())
        });

        match result {
            Ok(_) => Ok(()),
            Err(retry::Error { error, .. }) => Err(error),
        }?;

        if self.reconcile_gateway_cert_refs {
            match crate::cmd::kubectl::kubectl_reconcile_gateway_certrefs_for_router_tls_secrets(
                &kube_client,
                namespace.as_str(),
                gateway_name,
                "https",
            ) {
                Ok(true) => {
                    tracing::info!("Gateway certificateRefs reconciled for {}/{}", namespace, gateway_name);
                }
                Ok(false) => {
                    tracing::info!("Gateway certificateRefs already up to date for {}/{}", namespace, gateway_name);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to reconcile Gateway certificateRefs for {}/{}: {}",
                        namespace,
                        gateway_name,
                        e
                    );
                }
            }
        }

        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use ipnet::IpNet;

    use crate::environment::models::domain::Domain;
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::qovery_cluster_gateway_chart::{
        QoveryClusterGatewayChart, QoveryClusterGatewayChartOptions, XForwardedForClientIpDetection,
    };
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::cloud_provider::io::AwsAlbLoadBalancerScheme;
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
                load_balancer_source_ranges: vec![],
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
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
                load_balancer_source_ranges: vec![],
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
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
                load_balancer_source_ranges: vec![],
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
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

    #[test]
    fn x_forwarded_for_client_ip_detection_none_when_not_configured() {
        let detection = XForwardedForClientIpDetection::from_trusted_cidrs_and_hops(&[], None);
        assert!(matches!(detection, XForwardedForClientIpDetection::None));
    }

    #[test]
    fn x_forwarded_for_client_ip_detection_uses_trusted_hops_when_only_hops_is_set() {
        let detection = XForwardedForClientIpDetection::from_trusted_cidrs_and_hops(&[], Some(2));
        assert!(matches!(detection, XForwardedForClientIpDetection::TrustedHops(2)));
    }

    #[test]
    fn x_forwarded_for_client_ip_detection_uses_trusted_cidrs_when_only_cidrs_is_set() {
        let cidrs = vec![
            IpNet::V4("10.0.0.0/8".parse().unwrap_or_default()),
            IpNet::V4("192.168.0.0/16".parse().unwrap_or_default()),
        ];
        let detection = XForwardedForClientIpDetection::from_trusted_cidrs_and_hops(&cidrs, None);
        assert!(matches!(
            detection,
            XForwardedForClientIpDetection::TrustedCIDRs(returned) if returned == cidrs
        ));
    }

    #[test]
    fn x_forwarded_for_client_ip_detection_prefers_trusted_cidrs_when_both_are_set() {
        // Envoy Gateway enforces one-of semantics between trustedCIDRs and numTrustedHops.
        // We prioritize CIDRs because they are more explicit and resilient to hop-count drift.
        let cidrs = vec![IpNet::V4("10.0.0.0/8".parse().unwrap_or_default())];
        let detection = XForwardedForClientIpDetection::from_trusted_cidrs_and_hops(&cidrs, Some(3));
        assert!(matches!(
            detection,
            XForwardedForClientIpDetection::TrustedCIDRs(returned) if returned == cidrs
        ));
    }
}
