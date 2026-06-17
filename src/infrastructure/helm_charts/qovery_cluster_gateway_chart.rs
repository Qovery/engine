use crate::environment::models::domain::Domain;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces, HpaConfig,
};
use crate::infrastructure::helm_charts::envoy::access_log_format;
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
pub enum EnvoyGatewayApiPathEscapedSlashesAction {
    KeepUnchanged,      // Preserve %2F as-is in the upstream path.
    RejectRequest,      // Reject requests containing escaped slashes.
    UnescapeAndForward, // Decode %2F to / and forward upstream.
    #[default]
    UnescapeAndRedirect, // Decode %2F and redirect client to normalized path.
}

impl std::fmt::Display for EnvoyGatewayApiPathEscapedSlashesAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            EnvoyGatewayApiPathEscapedSlashesAction::KeepUnchanged => "KeepUnchanged",
            EnvoyGatewayApiPathEscapedSlashesAction::RejectRequest => "RejectRequest",
            EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndForward => "UnescapeAndForward",
            EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndRedirect => "UnescapeAndRedirect",
        };
        write!(f, "{value}")
    }
}

#[derive(Default)]
pub struct QoveryClusterGatewayChartOptions {
    pub dns_cloudflare_proxied: bool, // render provider-specific Cloudflare proxy annotation on bootstrap Gateway routes
    pub x_forwarded_for_client_ip_detection: XForwardedForClientIpDetection, // https://gateway.envoyproxy.io/v1.4/tasks/traffic/client-traffic-policy/#configure-client-ip-detection
    pub http_stream_idle_timeout_seconds: Option<u32>, // stream idle timeout for downstream HTTP streams
    pub path_disable_merge_slashes: bool, // preserve duplicate slashes behavior at gateway-level path handling
    pub path_escaped_slashes_action: EnvoyGatewayApiPathEscapedSlashesAction, // escaped slash handling action for gateway-level path handling
    pub custom_http_errors_default: Option<String>, // comma-separated HTTP status codes for gateway-level custom error pages
    pub compression_enable: bool, // enable response compression (brotli quality=6 and gzip level=6, matching nginx defaults)
    pub default_backend_enable: bool, // enable default backend deployment (matches nginx defaultBackend.enabled)
    pub default_backend_image: Option<String>, // default backend container image (e.g., "registry.k8s.io/ingress-nginx/custom-error-pages")
    pub default_backend_tag: Option<String>,   // default backend container image tag (e.g., "v1.1.1")
    pub hpa_config: Option<HpaConfig>,         // HPA for the Gateway-level EnvoyProxy that owns the data plane
    pub access_log_format: Option<String>,     // custom JSON access log format to apply on cluster-level EnvoyProxy
    pub reconcile_gateway_cert_refs: bool,     // reconcile Gateway TLS certificateRefs post-install/upgrade
}

pub struct QoveryClusterGatewayChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    additional_chart_path: Option<HelmChartValuesFilePath>,
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
        karpenter_enabled: bool,
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
            additional_chart_path: match karpenter_enabled {
                true => Some(HelmChartValuesFilePath::new(
                    chart_prefix_path,
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    format!("{}-with-karpenter", QoveryClusterGatewayChart::chart_name()),
                )),
                false => None,
            },
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
        let mut values_files = vec![self.chart_values_path.to_string()];
        if let Some(additional_chart_path) = &self.additional_chart_path {
            values_files.push(additional_chart_path.to_string());
        }

        let mut values_string = vec![];
        let mut chart_set_values = vec![ChartSetValue {
            key: "dns.domain".to_string(),
            value: self.domain.wildcarded().to_string(),
        }];
        chart_set_values.push(ChartSetValue {
            key: "dns.cloudflareProxied".to_string(),
            value: self.chart_options.dns_cloudflare_proxied.to_string(),
        });

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
        chart_set_values.push(ChartSetValue {
            key: "gateway.qoveryPublic.path.disableMergeSlashes".to_string(),
            value: self.chart_options.path_disable_merge_slashes.to_string(),
        });
        chart_set_values.push(ChartSetValue {
            key: "gateway.qoveryPublic.path.escapedSlashesAction".to_string(),
            value: self.chart_options.path_escaped_slashes_action.to_string(),
        });

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

        if let Some(hpa_config) = &self.chart_options.hpa_config {
            chart_set_values.push(ChartSetValue {
                key: "envoyProxy.qoveryPublic.provider.kubernetes.envoyHpa.minReplicas".to_string(),
                value: hpa_config.min_replicas.to_string(),
            });
            chart_set_values.push(ChartSetValue {
                key: "envoyProxy.qoveryPublic.provider.kubernetes.envoyHpa.maxReplicas".to_string(),
                value: hpa_config.max_replicas.to_string(),
            });

            let mut metric_index = 0;
            if let Some(cpu) = &hpa_config.cpu_average_utilization_percentage {
                let metric_prefix =
                    format!("envoyProxy.qoveryPublic.provider.kubernetes.envoyHpa.metrics[{metric_index}]");
                chart_set_values.push(ChartSetValue {
                    key: format!("{metric_prefix}.type"),
                    value: "Resource".to_string(),
                });
                chart_set_values.push(ChartSetValue {
                    key: format!("{metric_prefix}.resource.name"),
                    value: "cpu".to_string(),
                });
                chart_set_values.push(ChartSetValue {
                    key: format!("{metric_prefix}.resource.target.type"),
                    value: "Utilization".to_string(),
                });
                chart_set_values.push(ChartSetValue {
                    key: format!("{metric_prefix}.resource.target.averageUtilization"),
                    value: cpu.as_u8_percent().to_string(),
                });
                metric_index += 1;
            }

            if let Some(memory) = &hpa_config.memory_average_utilization_percentage {
                let metric_prefix =
                    format!("envoyProxy.qoveryPublic.provider.kubernetes.envoyHpa.metrics[{metric_index}]");
                chart_set_values.push(ChartSetValue {
                    key: format!("{metric_prefix}.type"),
                    value: "Resource".to_string(),
                });
                chart_set_values.push(ChartSetValue {
                    key: format!("{metric_prefix}.resource.name"),
                    value: "memory".to_string(),
                });
                chart_set_values.push(ChartSetValue {
                    key: format!("{metric_prefix}.resource.target.type"),
                    value: "Utilization".to_string(),
                });
                chart_set_values.push(ChartSetValue {
                    key: format!("{metric_prefix}.resource.target.averageUtilization"),
                    value: memory.as_u8_percent().to_string(),
                });
            }
        }

        let encoded_format = self
            .chart_options
            .access_log_format
            .as_ref()
            .map(|f| f.trim())
            .filter(|f| !f.is_empty())
            .map(|f| access_log_format::encode_envoy_access_log_format(&Self::chart_name(), f))
            .transpose()?
            .unwrap_or_default();
        values_string.push(ChartSetValue {
            key: "envoyProxy.qoveryPublic.accessLog.format".to_string(),
            value: encoded_format,
        });

        if let Some(annotations) = self.load_balancer.annotations() {
            for (key, value) in annotations {
                chart_set_values.push(ChartSetValue {
                    // Escape dots in annotation keys to prevent Helm from treating them as nested maps
                    key: format!(
                        "envoyProxy.qoveryPublic.provider.kubernetes.envoyService.annotations.{}",
                        key.replace('.', "\\.")
                    ),
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
                values_files,
                values: chart_set_values,
                values_string,
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
    use crate::helm::{HelmChartNamespaces, HpaConfig};
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
    use std::collections::HashSet;
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
                load_balancer_eip_allocation_ids: None,
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
                aws_apn_id: "pc:test-apn".to_string(),
            }),
            QoveryClusterGatewayChartOptions::default(),
            false,
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
                load_balancer_eip_allocation_ids: None,
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
                aws_apn_id: "pc:test-apn".to_string(),
            }),
            QoveryClusterGatewayChartOptions::default(),
            false,
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
                load_balancer_eip_allocation_ids: None,
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
                aws_apn_id: "pc:test-apn".to_string(),
            }),
            QoveryClusterGatewayChartOptions::default(),
            false,
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

    #[test]
    fn load_balancer_annotations_are_rendered_under_envoy_proxy_annotations() {
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            LoadBalancer::AwsAlb(AwsAlbLoadBalancer {
                cluster_id: QoveryIdentifier::new_random(),
                organization_id: QoveryIdentifier::new_random(),
                load_balancer_source_ranges: vec![],
                load_balancer_eip_allocation_ids: None,
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
                aws_apn_id: "pc:test-apn".to_string(),
            }),
            QoveryClusterGatewayChartOptions::default(),
            false,
            false,
        );

        let common_chart = chart.to_common_helm_chart().expect("chart should render");
        let keys: HashSet<&str> = common_chart
            .chart_info
            .values
            .iter()
            .map(|entry| entry.key.as_str())
            .collect();

        assert!(
            keys.iter().any(|key| {
                key.starts_with("envoyProxy.qoveryPublic.provider.kubernetes.envoyService.annotations.")
            }),
            "expected at least one envoy service annotation key"
        );
        assert!(
            keys.iter().all(|key| !key.starts_with("infrastructure.annotations.")),
            "gateway infrastructure annotations should no longer be used for LB service annotations"
        );
    }

    #[test]
    fn hpa_config_is_rendered_under_gateway_level_envoy_proxy() {
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            LoadBalancer::AwsAlb(AwsAlbLoadBalancer {
                cluster_id: QoveryIdentifier::new_random(),
                organization_id: QoveryIdentifier::new_random(),
                load_balancer_source_ranges: vec![],
                load_balancer_eip_allocation_ids: None,
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
                aws_apn_id: "pc:test-apn".to_string(),
            }),
            QoveryClusterGatewayChartOptions {
                hpa_config: Some(HpaConfig {
                    min_replicas: 2,
                    max_replicas: 25,
                    ..Default::default()
                }),
                ..Default::default()
            },
            false,
            false,
        );

        let common_chart = chart.to_common_helm_chart().expect("chart should render");
        let values: HashSet<(&str, &str)> = common_chart
            .chart_info
            .values
            .iter()
            .map(|entry| (entry.key.as_str(), entry.value.as_str()))
            .collect();

        assert!(values.contains(&("envoyProxy.qoveryPublic.provider.kubernetes.envoyHpa.minReplicas", "2")));
        assert!(values.contains(&("envoyProxy.qoveryPublic.provider.kubernetes.envoyHpa.maxReplicas", "25")));
    }

    #[test]
    fn custom_access_log_format_is_encoded_for_cluster_gateway_envoy_proxy() {
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            LoadBalancer::AwsAlb(AwsAlbLoadBalancer {
                cluster_id: QoveryIdentifier::new_random(),
                organization_id: QoveryIdentifier::new_random(),
                load_balancer_source_ranges: vec![],
                load_balancer_eip_allocation_ids: None,
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
                aws_apn_id: "pc:test-apn".to_string(),
            }),
            QoveryClusterGatewayChartOptions {
                access_log_format: Some(r#"{"correlation_id":"%REQ(X-REQUEST-ID)%"}"#.to_string()),
                ..Default::default()
            },
            false,
            false,
        );

        let common_chart = chart.to_common_helm_chart().expect("chart should render");
        let access_log_entry = common_chart
            .chart_info
            .values_string
            .iter()
            .find(|entry| entry.key == "envoyProxy.qoveryPublic.accessLog.format")
            .expect("access log format value should be set");

        assert!(!access_log_entry.value.is_empty(), "access log format should be base64 encoded");
    }

    #[test]
    fn cloudflare_proxy_setting_is_rendered_for_bootstrap_gateway_dns_route() {
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            LoadBalancer::AwsAlb(AwsAlbLoadBalancer {
                cluster_id: QoveryIdentifier::new_random(),
                organization_id: QoveryIdentifier::new_random(),
                load_balancer_source_ranges: vec![],
                load_balancer_eip_allocation_ids: None,
                load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
                aws_apn_id: "pc:test-apn".to_string(),
            }),
            QoveryClusterGatewayChartOptions {
                dns_cloudflare_proxied: true,
                ..Default::default()
            },
            false,
            false,
        );

        let common_chart = chart.to_common_helm_chart().expect("chart should render");
        let values: HashSet<(&str, &str)> = common_chart
            .chart_info
            .values
            .iter()
            .map(|entry| (entry.key.as_str(), entry.value.as_str()))
            .collect();

        assert!(values.contains(&("dns.cloudflareProxied", "true")));
    }
}
