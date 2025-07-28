use chrono::{DateTime, NaiveDateTime, Utc};
use chrono::{NaiveDate, NaiveTime};
use itertools::Itertools;
use std::fmt::Display;
use std::sync::Arc;
use strum_macros::EnumIter;

use super::{HelmChartResources, HelmChartResourcesConstraintType};
use crate::environment::models::domain::Domain;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, ChartValuesGenerated, CommonChart, HelmChartError,
    HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::cloud_provider::Kind;
use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
use crate::infrastructure::models::kubernetes::Kind::EksAnywhere;
use crate::io_models::models::{CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use kube::Client;
use reqwest::StatusCode;
use tera::{Context, Tera};

pub const NGINX_ADMISSION_CONTROLLER_STARTING_DATE: NaiveDateTime = NaiveDateTime::new(
    NaiveDate::from_ymd_opt(2024, 2, 1).expect("Invalid date on NGINX_ADMISSION_CONTROLLER_STARTING_DATE"),
    NaiveTime::from_hms_opt(0, 0, 0).expect("Invalid time on NGINX_ADMISSION_CONTROLLER_STARTING_DATE"),
);

#[derive(Clone)]
pub enum LogFormat {
    Default,
    Custom(String),
}

#[derive(Clone, EnumIter)]
pub enum LogFormatEscaping {
    Default,
    None,
    JSON,
}

impl Display for LogFormatEscaping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogFormatEscaping::Default => write!(f, "default"),
            LogFormatEscaping::None => write!(f, "none"),
            LogFormatEscaping::JSON => write!(f, "json"),
        }
    }
}

// TODO(bchastanier): this should probably be structured better than a string in the future
#[derive(Clone)]
pub struct NginxHttpSnippet(String);

impl NginxHttpSnippet {
    pub fn new(snippet: String) -> Self {
        NginxHttpSnippet(snippet)
    }

    pub fn get_snippet_value(&self) -> &str {
        &self.0
    }
}

// TODO(bchastanier): this should probably be structured better than a string in the future
#[derive(Clone)]
pub struct NginxConfigurationSnippet(String);

impl NginxConfigurationSnippet {
    pub fn new(snippet: String) -> Self {
        NginxConfigurationSnippet(snippet)
    }

    pub fn get_snippet_value(&self) -> &str {
        &self.0
    }
}

// TODO(bchastanier): this should probably be structured better than a string in the future
#[derive(Clone)]
pub struct NginxServerSnippet(String);

impl NginxServerSnippet {
    pub fn new(snippet: String) -> Self {
        NginxServerSnippet(snippet)
    }

    pub fn get_snippet_value(&self) -> &str {
        &self.0
    }
}

pub struct NginxLimitRequestStatusCode(StatusCode);

impl NginxLimitRequestStatusCode {
    pub fn new(status_code: StatusCode) -> Self {
        NginxLimitRequestStatusCode(status_code)
    }

    pub fn as_u16(&self) -> u16 {
        self.0.as_u16()
    }
}

pub struct NginxIngressChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    controller_resources: HelmChartResources,
    default_backend_resources: HelmChartResources,
    ff_metrics_history_enabled: bool,
    domain: Domain,
    cloud_provider: Kind,
    organization_long_id: String,
    organization_short_id: String,
    cluster_long_id: String,
    cluster_short_id: String,
    kubernetes_kind: KubernetesKind,
    customer_helm_chart_override: Option<CustomerHelmChartsOverride>,
    nginx_hpa_minimum_replicas: Option<u32>,
    nginx_hpa_maximum_replicas: Option<u32>,
    nginx_hpa_target_cpu_utilization_percentage: Option<u32>,
    namespace: HelmChartNamespaces,
    loadbalancer_size: Option<String>,
    enable_real_ip: bool,
    use_forwarded_headers: bool,
    compute_full_forwarded_for: bool,
    log_format_escaping: LogFormatEscaping,
    is_alb_enabled: bool,
    http_snippet: Option<NginxHttpSnippet>,
    server_snippet: Option<NginxServerSnippet>,
    limit_request_status_code: Option<NginxLimitRequestStatusCode>,
    nginx_controller_custom_http_errors: Option<String>,
    nginx_default_backend_enabled: Option<bool>,
    nginx_default_backend_image_repository: Option<String>,
    nginx_default_backend_image_tag: Option<String>,
    enable_admission_controller: bool,
    default_ssl_certificate: Option<String>,
    publish_status_address: Option<String>,
    replica_count: Option<u8>,
    metal_lb_load_balancer_ip: Option<String>,
    external_dns_target: Option<String>,
}

pub struct NginxOptions {
    pub nginx_hpa_minimum_replicas: Option<u32>,
    pub nginx_hpa_maximum_replicas: Option<u32>,
    pub nginx_hpa_target_cpu_utilization_percentage: Option<u32>,
    pub namespace: HelmChartNamespaces,
    pub loadbalancer_size: Option<String>,
    pub enable_real_ip: bool,
    pub use_forwarded_headers: bool,
    pub compute_full_forwarded_for: bool,
    pub log_format_escaping: LogFormatEscaping,
    pub is_alb_enabled: bool,
    pub http_snippet: Option<NginxHttpSnippet>,
    pub server_snippet: Option<NginxServerSnippet>,
    pub limit_request_status_code: Option<NginxLimitRequestStatusCode>,
    pub nginx_controller_custom_http_errors: Option<String>,
    pub nginx_default_backend_enabled: Option<bool>,
    pub nginx_default_backend_image_repository: Option<String>,
    pub nginx_default_backend_image_tag: Option<String>,
    pub default_ssl_certificate: Option<String>,
    pub publish_status_address: Option<String>,
    pub replica_count: Option<u8>,
    pub metal_lb_load_balancer_ip: Option<String>,
    pub external_dns_target: Option<String>,
}

impl NginxIngressChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        controller_resources: HelmChartResourcesConstraintType,
        default_backend_resources: HelmChartResourcesConstraintType,
        ff_metrics_history_enabled: bool,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        domain: Domain,
        cloud_provider: Kind,
        organization_long_id: String,
        organization_short_id: String,
        cluster_long_id: String,
        cluster_short_id: String,
        kubernetes_kind: KubernetesKind,
        created_cluster_date: DateTime<Utc>,
        options: NginxOptions,
    ) -> Self {
        NginxIngressChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                NginxIngressChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                NginxIngressChart::chart_name(),
            ),
            controller_resources: match controller_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(768),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(700),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(768),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            default_backend_resources: match default_backend_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(10),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(32),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(20),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(32),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            ff_metrics_history_enabled,
            domain,
            cloud_provider,
            kubernetes_kind,
            organization_long_id,
            organization_short_id,
            cluster_long_id,
            cluster_short_id,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            nginx_hpa_minimum_replicas: options.nginx_hpa_minimum_replicas,
            nginx_hpa_maximum_replicas: options.nginx_hpa_maximum_replicas,
            nginx_hpa_target_cpu_utilization_percentage: options.nginx_hpa_target_cpu_utilization_percentage,
            namespace: options.namespace,
            loadbalancer_size: options.loadbalancer_size,
            enable_real_ip: options.enable_real_ip,
            use_forwarded_headers: options.use_forwarded_headers,
            compute_full_forwarded_for: options.compute_full_forwarded_for,
            log_format_escaping: options.log_format_escaping,
            is_alb_enabled: options.is_alb_enabled,
            http_snippet: options.http_snippet,
            server_snippet: options.server_snippet,
            limit_request_status_code: options.limit_request_status_code,
            nginx_controller_custom_http_errors: options.nginx_controller_custom_http_errors,
            nginx_default_backend_enabled: options.nginx_default_backend_enabled,
            nginx_default_backend_image_repository: options.nginx_default_backend_image_repository,
            nginx_default_backend_image_tag: options.nginx_default_backend_image_tag,
            enable_admission_controller: Self::enable_admission_controller(&created_cluster_date),
            default_ssl_certificate: options.default_ssl_certificate,
            publish_status_address: options.publish_status_address,
            replica_count: options.replica_count,
            metal_lb_load_balancer_ip: options.metal_lb_load_balancer_ip,
            external_dns_target: options.external_dns_target,
        }
    }

    pub fn chart_name() -> String {
        "ingress-nginx".to_string()
    }

    // for history reasons where nginx-ingress has changed to ingress-nginx
    pub fn chart_old_name() -> String {
        "nginx-ingress".to_string()
    }

    pub fn enable_admission_controller(created_cluster_date: &DateTime<Utc>) -> bool {
        // admission controller should not be enabled for clusters created before this date
        // to avoid breaking changes during application deployments
        let start_date_to_enable_admission_controller =
            DateTime::<Utc>::from_naive_utc_and_offset(NGINX_ADMISSION_CONTROLLER_STARTING_DATE, Utc);

        *created_cluster_date >= start_date_to_enable_admission_controller
    }
}

impl ToCommonHelmChart for NginxIngressChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        // use this to override chart values but let the user to override it if necessary
        let mut tera = Tera::default();
        let nginx_ingress_override = r"
controller:
    resources:
        limits:
            cpu: {{ controller_resources_limits_cpu }}
            memory: {{ controller_resources_limits_memory }}
        requests:
            cpu: {{ controller_resources_requests_cpu }}
            memory: {{ controller_resources_requests_memory }}
defaultBackend:
    resources:
        limits:
            cpu: {{ default_backend_resources_limits_cpu }}
            memory: {{ default_backend_resources_limits_memory }}
        requests:
            cpu: {{ default_backend_resources_requests_cpu }}
            memory: {{ default_backend_resources_requests_memory }}
        ";
        tera.add_raw_template("nginx_ingress_override", nginx_ingress_override)
            .map_err(|e| HelmChartError::CreateTemplateError {
                chart_name: NginxIngressChart::chart_name(),
                msg: e.to_string(),
            })?;
        let mut context = Context::new();
        context.insert(
            "controller_resources_limits_cpu",
            &self.controller_resources.limit_cpu.to_string(),
        );
        context.insert(
            "controller_resources_limits_memory",
            &self.controller_resources.limit_memory.to_string(),
        );
        context.insert(
            "controller_resources_requests_cpu",
            &self.controller_resources.request_cpu.to_string(),
        );
        context.insert(
            "controller_resources_requests_memory",
            &self.controller_resources.request_memory.to_string(),
        );
        context.insert(
            "default_backend_resources_limits_cpu",
            &self.default_backend_resources.limit_cpu.to_string(),
        );
        context.insert(
            "default_backend_resources_limits_memory",
            &self.default_backend_resources.limit_memory.to_string(),
        );
        context.insert(
            "default_backend_resources_requests_cpu",
            &self.default_backend_resources.request_cpu.to_string(),
        );
        context.insert(
            "default_backend_resources_requests_memory",
            &self.default_backend_resources.request_memory.to_string(),
        );
        let rendered_nginx_override = ChartValuesGenerated::new(
            "qovery_nginx_ingress".to_string(),
            tera.render("nginx_ingress_override", &context)
                .map_err(|e| HelmChartError::RenderingError {
                    chart_name: NginxIngressChart::chart_name(),
                    msg: e.to_string(),
                })?,
        );

        let mut chart_set_values_string = vec![]; // Holding string values
        let mut chart_set_values = vec![
            ChartSetValue {
                key: "controller.allowSnippetAnnotations".to_string(),
                value: true.to_string(),
            },
            ChartSetValue {
                key: "controller.admissionWebhooks.enabled".to_string(),
                value: self.enable_admission_controller.to_string(),
            },
            // enable metrics for customers who want to manage it by their own
            ChartSetValue {
                key: "controller.metrics.enabled".to_string(),
                value: true.to_string(),
            },
            ChartSetValue {
                key: "controller.metrics.serviceMonitor.enabled".to_string(),
                value: self.ff_metrics_history_enabled.to_string(),
            },
            ChartSetValue {
                key: "controller.autoscaling.enabled".to_string(),
                value: true.to_string(),
            },
            ChartSetValue {
                key: "controller.config.enable-real-ip".to_string(),
                value: self.enable_real_ip.to_string(),
            },
            ChartSetValue {
                key: "controller.config.use-forwarded-headers".to_string(),
                value: self.use_forwarded_headers.to_string(),
            },
            ChartSetValue {
                key: "controller.config.compute-full-forwarded-for".to_string(),
                value: self.compute_full_forwarded_for.to_string(),
            },
        ];

        if let Some(value) = self.nginx_hpa_minimum_replicas {
            chart_set_values.push(ChartSetValue {
                key: "controller.autoscaling.minReplicas".to_string(),
                value: value.to_string(),
            })
        }
        if let Some(value) = self.nginx_hpa_maximum_replicas {
            chart_set_values.push(ChartSetValue {
                key: "controller.autoscaling.maxReplicas".to_string(),
                value: value.to_string(),
            })
        }
        if let Some(value) = self.nginx_hpa_target_cpu_utilization_percentage {
            chart_set_values.push(ChartSetValue {
                key: "controller.autoscaling.targetCPUUtilizationPercentage".to_string(),
                value: value.to_string(),
            })
        }
        if let Some(value) = &self.limit_request_status_code {
            chart_set_values_string.push(ChartSetValue {
                key: "controller.config.limit-req-status-code".to_string(),
                value: value.as_u16().to_string(),
            })
        }
        if let Some(value) = &self.http_snippet {
            chart_set_values_string.push(ChartSetValue {
                key: "controller.config.http-snippet".to_string(),
                value: value.get_snippet_value().to_string(),
            })
        }
        if let Some(value) = &self.server_snippet {
            chart_set_values_string.push(ChartSetValue {
                key: "controller.config.server-snippet".to_string(),
                value: value.get_snippet_value().to_string(),
            })
        }
        match self.log_format_escaping {
            LogFormatEscaping::None => {
                chart_set_values.push(ChartSetValue {
                    key: "controller.config.log-format-escaping-none".to_string(),
                    value: true.to_string(),
                });
            }
            LogFormatEscaping::JSON => {
                chart_set_values.push(ChartSetValue {
                    key: "controller.config.log-format-escaping-json".to_string(),
                    value: true.to_string(),
                });
            }
            LogFormatEscaping::Default => {}
        }

        if let Some(nginx_default_backend_enabled) = self.nginx_default_backend_enabled {
            chart_set_values.push(ChartSetValue {
                key: "defaultBackend.enabled".to_string(),
                value: nginx_default_backend_enabled.to_string(),
            });

            if self.nginx_default_backend_image_repository.is_none() && nginx_default_backend_enabled {
                // the default image will be used and this image support only amd arch
                chart_set_values.push(ChartSetValue {
                    key: "defaultBackend.nodeSelector.kubernetes\\.io/arch".to_string(),
                    value: "amd64".to_string(),
                });
            }
        }

        if let Some(nginx_controller_custom_http_errors) = &self.nginx_controller_custom_http_errors {
            chart_set_values.push(ChartSetValue {
                key: "controller.config.custom-http-errors".to_string(),
                value: nginx_controller_custom_http_errors
                    .split(",")
                    .map(|s| s.trim())
                    .join("\\,"),
            });
        }

        if let Some(nginx_default_backend_image_repository) = &self.nginx_default_backend_image_repository {
            chart_set_values.push(ChartSetValue {
                key: "defaultBackend.image.repository".to_string(),
                value: nginx_default_backend_image_repository.clone(),
            });
        }

        if let Some(nginx_default_backend_image_tag) = &self.nginx_default_backend_image_tag {
            chart_set_values.push(ChartSetValue {
                key: "defaultBackend.image.tag".to_string(),
                value: nginx_default_backend_image_tag.clone(),
            });
        }

        // custom cloud provider configuration
        match self.cloud_provider {
            Kind::Aws => {
                // there is no LB deployed for EC2
                if self.kubernetes_kind == KubernetesKind::Eks {
                    // common config
                    chart_set_values.push(ChartSetValue {
                        key: "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-healthcheck-interval"
                            .to_string(),
                        value: "10".to_string(),
                    });

                    // alb controller VS native k8s nlb
                    match self.is_alb_enabled {
                        true => {
                            chart_set_values.push(ChartSetValue {
                                key: "controller.config.use-proxy-protocol".to_string(),
                                value: "true".to_string(),
                            });
                            chart_set_values.push(ChartSetValue {
                                key:
                                "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-name"
                                    .to_string(),
                                value: format!("qovery-{}-nginx-ingress", self.cluster_short_id),
                            });
                            chart_set_values.push(ChartSetValue {
                                key:
                                "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-type"
                                    .to_string(),
                                value: "external".to_string(),
                            });
                            chart_set_values.push(ChartSetValue {
                                key: "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-scheme"
                                    .to_string(),
                                value: "internet-facing".to_string(),
                            });
                            chart_set_values.push(ChartSetValue {
                                key: "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-proxy-protocol".to_string(),
                                value: "*".to_string(),
                            });
                            chart_set_values.push(ChartSetValue {
                                key: "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-nlb-target-type".to_string(),
                                value: "ip".to_string(),
                            });
                            chart_set_values.push(ChartSetValue {
                                key: "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-cross-zone-load-balancing-enabled".to_string(),
                                value: "true".to_string(),
                            });
                            chart_set_values.push(ChartSetValue {
                                key: "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-target-group-attributes".to_string(),
                                value: "target_health_state\\.unhealthy\\.connection_termination\\.enabled=false,target_health_state\\.unhealthy\\.draining_interval_seconds=300".to_string(),
                            });
                            chart_set_values.push(ChartSetValue {
                                key:
                                "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-additional-resource-tags"
                                    .to_string(),
                                value: format!(
                                    "OrganizationLongId={}\\,OrganizationId={}\\,ClusterLongId={}\\,ClusterId={}",
                                    self.organization_long_id,
                                    self.organization_short_id,
                                    self.cluster_long_id,
                                    self.cluster_short_id,
                                ),
                            });
                        }
                        false => {
                            chart_set_values.push(ChartSetValue {
                                key:
                                "controller.service.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-type"
                                    .to_string(),
                                value: "nlb".to_string(),
                            });
                        }
                    };
                };
            }
            Kind::Scw => {
                chart_set_values.push(ChartSetValue {
                    key: "controller.service.annotations.service\\.beta\\.kubernetes\\.io/scw-loadbalancer-type"
                        .to_string(),
                    value: match self.loadbalancer_size.clone() {
                        Some(size) => size,
                        None => {
                            return Err(HelmChartError::RenderingError {
                                chart_name: NginxIngressChart::chart_name(),
                                msg: "scw-loadbalancer-type is required but information is missing".to_string(),
                            });
                        }
                    },
                });
            }
            Kind::Gcp => {}
            Kind::Azure => {}
            Kind::OnPremise => {
                if self.kubernetes_kind == EksAnywhere {
                    if let Some(value) = &self.default_ssl_certificate {
                        chart_set_values.push(ChartSetValue {
                            key: "controller.extraArgs.default-ssl-certificate".to_string(),
                            value: value.to_string(),
                        });
                    }
                    if let Some(value) = &self.publish_status_address {
                        chart_set_values.push(ChartSetValue {
                            key: "controller.extraArgs.publish-status-address".to_string(),
                            value: value.to_string(),
                        });
                    }
                    if let Some(value) = &self.replica_count {
                        chart_set_values.push(ChartSetValue {
                            key: "controller.replicaCount".to_string(),
                            value: value.to_string(),
                        });
                    }
                    if let Some(value) = &self.metal_lb_load_balancer_ip {
                        chart_set_values.push(ChartSetValue {
                            key: "controller.service.annotations.metallb\\.universe\\.tf/loadBalancerIPs".to_string(),
                            value: value.to_string(),
                        });
                    }
                    if let Some(value) = &self.external_dns_target {
                        chart_set_values.push(ChartSetValue {
                            key: "controller.service.annotations.external-dns\\.alpha\\.kubernetes\\.io/target"
                                .to_string(),
                            value: value.to_string(),
                        });
                    }
                }
            }
        }
        // external dns
        chart_set_values.push(ChartSetValue {
            key: "controller.service.annotations.external-dns\\.alpha\\.kubernetes\\.io/hostname".to_string(),
            value: self.domain.wildcarded().to_string(),
        });

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: NginxIngressChart::chart_old_name(),
                path: self.chart_path.to_string(),
                namespace: self.namespace.clone(),
                // Because of NLB, svc can take some time to start
                // rolling out the deployment can take a lot of time for users that has a lot of nginx
                timeout_in_seconds: 60 * 60,
                values_files: vec![self.chart_values_path.to_string()],
                values: chart_set_values,
                values_string: chart_set_values_string,
                yaml_files_content: {
                    // order matters: last one overrides previous ones, so customer override should be last
                    let mut x = vec![rendered_nginx_override];
                    if let Some(customer_helm_chart_override) = self.customer_helm_chart_override.clone() {
                        x.push(customer_helm_chart_override.to_chart_values_generated());
                    };
                    x
                },
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(NginxIngressChartChecker::new())),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct NginxIngressChartChecker {}

impl NginxIngressChartChecker {
    pub fn new() -> NginxIngressChartChecker {
        NginxIngressChartChecker {}
    }
}

impl Default for NginxIngressChartChecker {
    fn default() -> Self {
        NginxIngressChartChecker::new()
    }
}

impl ChartInstallationChecker for NginxIngressChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1370): Implement chart install verification
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
    use crate::infrastructure::helm_charts::HelmChartResourcesConstraintType;
    use crate::infrastructure::helm_charts::HelmChartType;
    use crate::infrastructure::helm_charts::ToCommonHelmChart;
    use crate::infrastructure::helm_charts::nginx_ingress_chart::NginxIngressChart;
    use crate::infrastructure::helm_charts::nginx_ingress_chart::{LogFormatEscaping, NginxOptions};
    use crate::infrastructure::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::cloud_provider::Kind;
    use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
    use crate::io_models::models::CustomerHelmChartsOverride;
    use chrono::TimeZone;
    use chrono::Utc;
    use std::env;
    use std::sync::Arc;
    use strum::IntoEnumIterator;

    fn get_nginx_ingress_chart_override() -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
        Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> {
            Some(CustomerHelmChartsOverride {
                chart_name: NginxIngressChart::chart_name(),
                chart_values: "".to_string(),
            })
        })
    }

    fn get_domain() -> Domain {
        Domain::new("qovery.com".to_string())
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn nginx_ingress_chart_directory_exists_test() {
        // setup:
        let chart = NginxIngressChart::new(
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            true,
            get_nginx_ingress_chart_override(),
            get_domain().wildcarded(),
            Kind::Aws,
            "00000000-0000-4000-8000-000000000000".to_string(),
            "z00000000".to_string(),
            "10000000-0000-4000-8000-000000000000".to_string(),
            "z10000000".to_string(),
            KubernetesKind::Eks,
            Utc::now(),
            NginxOptions {
                nginx_hpa_minimum_replicas: Some(1),
                nginx_hpa_maximum_replicas: Some(10),
                nginx_hpa_target_cpu_utilization_percentage: Some(50),
                namespace: HelmChartNamespaces::NginxIngress,
                loadbalancer_size: None,
                enable_real_ip: true,
                use_forwarded_headers: true,
                compute_full_forwarded_for: true,
                log_format_escaping: LogFormatEscaping::Default,
                is_alb_enabled: false,
                http_snippet: None,
                server_snippet: None,
                limit_request_status_code: None,
                nginx_controller_custom_http_errors: None,
                nginx_default_backend_enabled: None,
                nginx_default_backend_image_repository: None,
                nginx_default_backend_image_tag: None,
                default_ssl_certificate: None,
                publish_status_address: None,
                replica_count: None,
                metal_lb_load_balancer_ip: None,
                external_dns_target: None,
            },
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            NginxIngressChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    // Makes sure chart values file exists.
    // todo:(pmavro): fix it
    #[test]
    fn nginx_ingress_chart_values_file_exists_test() {
        // setup:
        let chart = NginxIngressChart::new(
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            true,
            get_nginx_ingress_chart_override(),
            get_domain().wildcarded(),
            Kind::Aws,
            "00000000-0000-4000-8000-000000000000".to_string(),
            "z00000000".to_string(),
            "10000000-0000-4000-8000-000000000000".to_string(),
            "z10000000".to_string(),
            KubernetesKind::Eks,
            Utc::now(),
            NginxOptions {
                nginx_hpa_minimum_replicas: Some(1),
                nginx_hpa_maximum_replicas: Some(10),
                nginx_hpa_target_cpu_utilization_percentage: Some(50),
                namespace: HelmChartNamespaces::NginxIngress,
                loadbalancer_size: None,
                enable_real_ip: true,
                use_forwarded_headers: true,
                compute_full_forwarded_for: true,
                log_format_escaping: LogFormatEscaping::Default,
                is_alb_enabled: false,
                http_snippet: None,
                server_snippet: None,
                limit_request_status_code: None,
                nginx_controller_custom_http_errors: None,
                nginx_default_backend_enabled: None,
                nginx_default_backend_image_repository: None,
                nginx_default_backend_image_tag: None,
                default_ssl_certificate: None,
                publish_status_address: None,
                replica_count: None,
                metal_lb_load_balancer_ip: None,
                external_dns_target: None,
            },
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.j2.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            NginxIngressChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    #[test]
    fn nginx_ingress_chart_log_format_escaping_test() {
        for log_format_escaping in LogFormatEscaping::iter() {
            // setup:
            let chart = NginxIngressChart::new(
                None,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartResourcesConstraintType::ChartDefault,
                true,
                get_nginx_ingress_chart_override(),
                get_domain().wildcarded(),
                Kind::Aws,
                "00000000-0000-4000-8000-000000000000".to_string(),
                "z00000000".to_string(),
                "10000000-0000-4000-8000-000000000000".to_string(),
                "z10000000".to_string(),
                KubernetesKind::Eks,
                Utc::now(),
                NginxOptions {
                    nginx_hpa_minimum_replicas: None,
                    nginx_hpa_maximum_replicas: None,
                    nginx_hpa_target_cpu_utilization_percentage: None,
                    namespace: HelmChartNamespaces::NginxIngress,
                    loadbalancer_size: None,
                    enable_real_ip: true,
                    use_forwarded_headers: true,
                    compute_full_forwarded_for: true,
                    log_format_escaping: log_format_escaping.clone(),
                    is_alb_enabled: false,
                    http_snippet: None,
                    server_snippet: None,
                    limit_request_status_code: None,
                    nginx_controller_custom_http_errors: None,
                    nginx_default_backend_enabled: None,
                    nginx_default_backend_image_repository: None,
                    nginx_default_backend_image_tag: None,
                    default_ssl_certificate: None,
                    publish_status_address: None,
                    replica_count: None,
                    metal_lb_load_balancer_ip: None,
                    external_dns_target: None,
                },
            );

            // execute:
            let common_chart = chart.to_common_helm_chart().expect("cannot create common chart");

            // verify:
            match &log_format_escaping {
                LogFormatEscaping::Default => {
                    assert!(
                        !common_chart
                            .chart_info
                            .values
                            .iter()
                            .any(|x| x.key == "controller.config.log-format-escaping-none"
                                || x.key == "controller.config.log-format-escaping-json")
                    );
                }
                _ => {
                    assert!(common_chart.chart_info.values.iter().any(|x| x.key
                        == format!("controller.config.log-format-escaping-{log_format_escaping}")
                        && x.value == "true"),);
                }
            }
        }
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    #[ignore = "TODO: fix it, removing or handling the jinja templating for proper testing"]
    fn nginx_ingress_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = NginxIngressChart::new(
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            true,
            get_nginx_ingress_chart_override(),
            get_domain().wildcarded(),
            Kind::Aws,
            "00000000-0000-4000-8000-000000000000".to_string(),
            "z00000000".to_string(),
            "10000000-0000-4000-8000-000000000000".to_string(),
            "z10000000".to_string(),
            KubernetesKind::Eks,
            Utc::now(),
            NginxOptions {
                nginx_hpa_minimum_replicas: Some(1),
                nginx_hpa_maximum_replicas: Some(10),
                nginx_hpa_target_cpu_utilization_percentage: Some(50),
                namespace: HelmChartNamespaces::NginxIngress,
                loadbalancer_size: None,
                enable_real_ip: true,
                use_forwarded_headers: true,
                compute_full_forwarded_for: true,
                log_format_escaping: LogFormatEscaping::Default,
                is_alb_enabled: false,
                http_snippet: None,
                server_snippet: None,
                limit_request_status_code: None,
                nginx_controller_custom_http_errors: None,
                nginx_default_backend_enabled: None,
                nginx_default_backend_image_repository: None,
                nginx_default_backend_image_tag: None,
                default_ssl_certificate: None,
                publish_status_address: None,
                replica_count: None,
                metal_lb_load_balancer_ip: None,
                external_dns_target: None,
            },
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
                ),
                NginxIngressChart::chart_name(),
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
    fn check_nginx_admission_controller_activation() {
        // should allow admission controller
        let now = NginxIngressChart::enable_admission_controller(&Utc::now());
        assert!(now);
        // should deny admission controller
        let old_date = NginxIngressChart::enable_admission_controller(&Utc.ymd(2023, 1, 1).and_hms(0, 0, 0));
        assert!(!old_date);
    }
}
