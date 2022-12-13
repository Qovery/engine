use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartResourcesConstraintType,
    HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::cloud_provider::models::{IngressLoadBalancerType, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::errors::CommandError;
use crate::io_models::domain::Domain;
use kube::Client;

pub struct NginxIngressChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    hostname: Option<Domain>,
    load_balancer_type: Option<Box<dyn IngressLoadBalancerType>>,
    chart_controller_resources: HelmChartResources,
    chart_default_backend_resources: HelmChartResources,
}

impl NginxIngressChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        hostname: Option<Domain>,
        load_balancer_type: Option<Box<dyn IngressLoadBalancerType>>,
        chart_controller_resources: HelmChartResourcesConstraintType,
        chart_default_backend_resources: HelmChartResourcesConstraintType,
    ) -> Self {
        NginxIngressChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                NginxIngressChart::chart_folder_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                NginxIngressChart::chart_name(),
            ),
            chart_controller_resources: match chart_controller_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(768),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(768),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            chart_default_backend_resources: match chart_default_backend_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(20),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(32),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(10),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(32),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            hostname,
            load_balancer_type,
        }
    }

    pub fn chart_name() -> String {
        "nginx-ingress".to_string()
    }

    // TODO(benjaminch): to be merged with naem at some point, this is legacy since nginx team renamed
    // chart from nginx-ingress to ingress-nginx
    pub fn chart_folder_name() -> String {
        "ingress-nginx".to_string()
    }
}

impl ToCommonHelmChart for NginxIngressChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        let mut chart = CommonChart {
            chart_info: ChartInfo {
                name: NginxIngressChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: HelmChartNamespaces::NginxIngress,
                // Because of NLB, svc can take some time to start
                timeout_in_seconds: 300,
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    // Controller resources limits
                    ChartSetValue {
                        key: "controller.resources.limits.cpu".to_string(),
                        value: self.chart_controller_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "controller.resources.limits.memory".to_string(),
                        value: self.chart_controller_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "controller.resources.requests.cpu".to_string(),
                        value: self.chart_controller_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "controller.resources.requests.memory".to_string(),
                        value: self.chart_controller_resources.request_memory.to_string(),
                    },
                    // Default backend resources limits
                    ChartSetValue {
                        key: "defaultBackend.resources.limits.cpu".to_string(),
                        value: self.chart_default_backend_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "defaultBackend.resources.limits.memory".to_string(),
                        value: self.chart_default_backend_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "defaultBackend.resources.requests.cpu".to_string(),
                        value: self.chart_default_backend_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "defaultBackend.resources.requests.memory".to_string(),
                        value: self.chart_default_backend_resources.request_memory.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(NginxIngressChartChecker::new())),
        };

        // Host DNS
        if let Some(hostname) = &self.hostname {
            chart.chart_info.values.push(ChartSetValue {
                key: r#"controller.service.annotations.external-dns\.alpha\.kubernetes\.io/hostname"#.to_string(),
                value: hostname.wildcarded().to_string(),
            });
        }

        // Load balancer type
        if let Some(load_balancer_type) = &self.load_balancer_type {
            chart.chart_info.values.push(ChartSetValue {
                key: format!(
                    "controller.service.annotations.{}",
                    load_balancer_type.annotation_key().replace('.', r#"\."#)
                ),
                value: load_balancer_type.annotation_value(),
            });
        }

        chart
    }
}

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
        // TODO(ENG-1407): Implement chart install verification
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm_charts::nginx_ingress_chart::NginxIngressChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartResourcesConstraintType, HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::kubernetes::Kind;
    use crate::cloud_provider::models::IngressLoadBalancerType;
    use crate::io_models::domain::Domain;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn nginx_ingress_chart_directory_exists_test() {
        // setup:
        let chart = NginxIngressChart::new(
            None,
            None,
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            NginxIngressChart::chart_folder_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }

    /// Makes sure chart values file exists.
    #[test]
    fn nginx_ingress_chart_values_file_exists_test() {
        // setup:
        let chart = NginxIngressChart::new(
            None,
            None,
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
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
            NginxIngressChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn nginx_ingress_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = NginxIngressChart::new(
            None,
            None,
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
        );
        let common_chart = chart.to_common_helm_chart();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(Kind::Eks),
                ),
                NginxIngressChart::chart_name(),
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }

    #[test]
    fn nginx_ingress_chart_load_balancer_type_annotation_properly_set() {
        // setup:
        struct FakeLoadBalancer {
            annotation: String,
            name: String,
        }
        impl IngressLoadBalancerType for FakeLoadBalancer {
            fn annotation_key(&self) -> String {
                self.annotation.to_string()
            }

            fn annotation_value(&self) -> String {
                self.name.to_string()
            }
        }

        struct TestCase {
            ingress_load_balancer_type: Option<Box<dyn IngressLoadBalancerType>>,
            expected_load_balancer_type_annotation_key: String,
            expected_load_balancer_type_annotation_value: String,
        }

        let test_cases = vec![
            TestCase {
                ingress_load_balancer_type: Some(Box::new(FakeLoadBalancer {
                    annotation: "service.beta.kubernetes.io/fake-load-balancer-type".to_string(),
                    name: "whatever-type".to_string(),
                })),
                expected_load_balancer_type_annotation_key:
                    r#"controller.service.annotations.service\.beta\.kubernetes\.io/fake-load-balancer-type"#
                        .to_string(),
                expected_load_balancer_type_annotation_value: "whatever-type".to_string(),
            },
            TestCase {
                ingress_load_balancer_type: Some(Box::new(FakeLoadBalancer {
                    annotation: "service.beta.kubernetes.io/fake-load-balancer-type.toto".to_string(),
                    name: "whatever-type.toto".to_string(),
                })),
                expected_load_balancer_type_annotation_key:
                    r#"controller.service.annotations.service\.beta\.kubernetes\.io/fake-load-balancer-type\.toto"#
                        .to_string(),
                expected_load_balancer_type_annotation_value: "whatever-type.toto".to_string(),
            },
        ];

        for tc in test_cases {
            // execute :
            let chart = NginxIngressChart::new(
                None,
                None,
                tc.ingress_load_balancer_type,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartResourcesConstraintType::ChartDefault,
            )
            .to_common_helm_chart();

            // verify:
            let mut value_found = false;
            let mut annotation_found = false;
            for v in chart.chart_info.values {
                if v.key == tc.expected_load_balancer_type_annotation_key {
                    annotation_found = true;

                    if v.value == tc.expected_load_balancer_type_annotation_value {
                        value_found = true;
                        break;
                    }

                    break;
                }
            }

            assert!(annotation_found && value_found);
        }
    }

    #[test]
    fn nginx_ingress_chart_hostname_dns_annotation_properly_set() {
        // setup:
        struct TestCase {
            hostname_dns: Option<Domain>,
            expected_hostname_dns: Option<String>,
        }

        let test_cases = vec![
            TestCase {
                hostname_dns: None,
                expected_hostname_dns: None,
            },
            TestCase {
                hostname_dns: Some(Domain::new("whatever.com".to_string())),
                expected_hostname_dns: Some("*.whatever.com".to_string()),
            },
            TestCase {
                hostname_dns: Some(Domain::new("*.whatever.com".to_string())),
                expected_hostname_dns: Some("*.whatever.com".to_string()),
            },
        ];

        for tc in test_cases {
            // execute :
            let chart = NginxIngressChart::new(
                None,
                tc.hostname_dns.clone(),
                None,
                HelmChartResourcesConstraintType::ChartDefault,
                HelmChartResourcesConstraintType::ChartDefault,
            )
            .to_common_helm_chart();

            // verify:
            let mut annotation_found = false;
            let mut hostname_value_found = false;
            for v in chart.chart_info.values {
                if v.key == *r#"controller.service.annotations.external-dns\.alpha\.kubernetes\.io/hostname"# {
                    annotation_found = true;
                    if tc.hostname_dns.is_some() {
                        if let Some(d) = &tc.expected_hostname_dns {
                            if *d == v.value {
                                hostname_value_found = true;
                                break;
                            }
                        }
                    }
                }
            }

            // we check annotation is found with the proper value set if needed
            // if hostname shouldn't be set, make sure it's not there
            match &tc.hostname_dns {
                Some(_) => assert!(annotation_found && hostname_value_found),
                None => assert!(!annotation_found),
            }
        }
    }
}
