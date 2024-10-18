use std::fmt::Display;
use std::sync::Arc;
use strum_macros::EnumIter;

use super::{HelmChartResources, HelmChartResourcesConstraintType};
use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, ChartValuesGenerated, CommonChart, HelmChartError,
    HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::kubernetes::Kind::Ec2;
use crate::cloud_provider::models::{
    CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit,
};
use crate::cloud_provider::Kind;
use crate::errors::CommandError;
use crate::models::domain::Domain;
use kube::Client;
use tera::{Context, Tera};

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
    log_format_escaping: LogFormatEscaping,
    is_alb_enabled: bool,
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
        nginx_hpa_minimum_replicas: Option<u32>,
        nginx_hpa_maximum_replicas: Option<u32>,
        nginx_hpa_target_cpu_utilization_percentage: Option<u32>,
        namespace: HelmChartNamespaces,
        loadbalancer_size: Option<String>,
        enable_real_ip: bool,
        log_format_escaping: LogFormatEscaping,
        is_alb_enabled: bool,
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
            nginx_hpa_minimum_replicas,
            nginx_hpa_maximum_replicas,
            nginx_hpa_target_cpu_utilization_percentage,
            namespace,
            loadbalancer_size,
            enable_real_ip,
            log_format_escaping,
            is_alb_enabled,
        }
    }

    pub fn chart_name() -> String {
        "ingress-nginx".to_string()
    }

    // for history reasons where nginx-ingress has changed to ingress-nginx
    pub fn chart_old_name() -> String {
        "nginx-ingress".to_string()
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

        let mut chart_set_values = vec![
            ChartSetValue {
                key: "controller.allowSnippetAnnotations".to_string(),
                value: true.to_string(),
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
                value: if self.kubernetes_kind == Ec2 {
                    false.to_string()
                } else {
                    true.to_string()
                },
            },
            ChartSetValue {
                key: "controller.config.enable-real-ip".to_string(),
                value: self.enable_real_ip.to_string(),
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
                            })
                        }
                    },
                });
            }
            Kind::Gcp => {}
            Kind::OnPremise => {}
        }
        // external dns
        if self.kubernetes_kind != KubernetesKind::Ec2 {
            chart_set_values.push(ChartSetValue {
                key: "controller.service.annotations.external-dns\\.alpha\\.kubernetes\\.io/hostname".to_string(),
                value: self.domain.wildcarded().to_string(),
            })
        };

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: NginxIngressChart::chart_old_name(),
                path: self.chart_path.to_string(),
                namespace: self.namespace,
                // Because of NLB, svc can take some time to start
                // rolling out the deployment can take a lot of time for users that has a lot of nginx
                timeout_in_seconds: 60 * 60,
                values_files: vec![self.chart_values_path.to_string()],
                values: chart_set_values,
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
    use crate::cloud_provider::helm::HelmChartNamespaces;
    use crate::cloud_provider::helm_charts::get_helm_path_kubernetes_provider_sub_folder_name;
    use crate::cloud_provider::helm_charts::nginx_ingress_chart::LogFormatEscaping;
    use crate::cloud_provider::helm_charts::nginx_ingress_chart::NginxIngressChart;
    use crate::cloud_provider::helm_charts::HelmChartResourcesConstraintType;
    use crate::cloud_provider::helm_charts::HelmChartType;
    use crate::cloud_provider::helm_charts::ToCommonHelmChart;
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use crate::cloud_provider::models::CustomerHelmChartsOverride;
    use crate::cloud_provider::Kind;
    use crate::models::domain::Domain;
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
            Some(1),
            Some(10),
            Some(50),
            HelmChartNamespaces::NginxIngress,
            None,
            false,
            LogFormatEscaping::Default,
            false,
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
            Some(1),
            Some(10),
            Some(50),
            HelmChartNamespaces::NginxIngress,
            None,
            false,
            LogFormatEscaping::Default,
            false,
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
                KubernetesKind::Ec2,
                None,
                None,
                None,
                HelmChartNamespaces::NginxIngress,
                None,
                false,
                log_format_escaping.clone(),
                false,
            );

            // execute:
            let common_chart = chart.to_common_helm_chart().expect("cannot create common chart");

            // verify:
            match &log_format_escaping {
                LogFormatEscaping::Default => {
                    assert!(!common_chart
                        .chart_info
                        .values
                        .iter()
                        .any(|x| x.key == "controller.config.log-format-escaping-none"
                            || x.key == "controller.config.log-format-escaping-json"));
                }
                _ => {
                    assert!(common_chart.chart_info.values.iter().any(|x| x.key
                        == format!("controller.config.log-format-escaping-{}", log_format_escaping)
                        && x.value == "true"),);
                }
            }
        }
    }
}
