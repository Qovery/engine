use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartCRDsPath, HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};

use super::{HelmChartResources, HelmChartResourcesConstraintType};
use crate::cmd::helm_utils::CRDSUpdate;
use crate::errors::CommandError;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use kube::Client;
use semver::Version;

pub struct VpaChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    helm_chart_crds_path: HelmChartCRDsPath,
    recommender_resources: HelmChartResources,
    updater_resources: HelmChartResources,
    admission_controller_resources: HelmChartResources,
    enable_vpa: bool,
    namespace: HelmChartNamespaces,
    skip_if_already_installed: bool,
}

impl VpaChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        recommended_resources: HelmChartResourcesConstraintType,
        updater_resources: HelmChartResourcesConstraintType,
        admission_controller_resources: HelmChartResourcesConstraintType,
        enable_vpa: bool,
        namespace: HelmChartNamespaces,
        skip_if_already_installed: bool,
    ) -> VpaChart {
        let chart_path = HelmChartPath::new(
            chart_prefix_path,
            HelmChartDirectoryLocation::CommonFolder,
            VpaChart::chart_name(),
        );

        VpaChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: chart_path.clone(),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                VpaChart::chart_name(),
            ),
            helm_chart_crds_path: HelmChartCRDsPath::new(chart_path, "crds/"),
            recommender_resources: match recommended_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    request_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            updater_resources: match updater_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    request_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            admission_controller_resources: match admission_controller_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(500),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(500),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            enable_vpa,
            namespace,
            skip_if_already_installed,
        }
    }

    fn chart_name() -> String {
        "vertical-pod-autoscaler".to_string()
    }
}

impl ToCommonHelmChart for VpaChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: "vertical-pod-autoscaler".to_string(),
                namespace: self.namespace,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    // recommender
                    ChartSetValue {
                        key: "recommender.resources.requests.cpu".to_string(),
                        value: self.recommender_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "recommender.resources.requests.memory".to_string(),
                        value: self.recommender_resources.request_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "recommender.resources.limits.cpu".to_string(),
                        value: self.recommender_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "recommender.resources.limits.memory".to_string(),
                        value: self.recommender_resources.limit_memory.to_string(),
                    },
                    // updater
                    ChartSetValue {
                        key: "updater.resources.requests.cpu".to_string(),
                        value: self.updater_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "updater.resources.requests.memory".to_string(),
                        value: self.updater_resources.request_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "updater.resources.limits.cpu".to_string(),
                        value: self.updater_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "updater.resources.limits.memory".to_string(),
                        value: self.updater_resources.limit_memory.to_string(),
                    },
                    // admission controller
                    ChartSetValue {
                        key: "admissionController.resources.requests.cpu".to_string(),
                        value: self.admission_controller_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "admissionController.resources.requests.memory".to_string(),
                        value: self.admission_controller_resources.request_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "admissionController.resources.limits.cpu".to_string(),
                        value: self.admission_controller_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "admissionController.resources.limits.memory".to_string(),
                        value: self.admission_controller_resources.limit_memory.to_string(),
                    },
                ],
                crds_update: Some(CRDSUpdate {
                    path: self.helm_chart_crds_path.to_string(),
                    resources: vec!["vpa-v1-crd.yaml".to_string()],
                }),
                skip_if_already_installed: self.skip_if_already_installed,
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(4, 6, 0)), // CRDs needs to reinstalled https://artifacthub.io/packages/helm/fairwinds-stable/vpa#breaking-upgrading-to-4-0-0
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(VpaChartInstallationChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "vertical-pod-autoscaler-vpa-admission-controller".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(1000)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(256)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                            ),
                        },
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "vertical-pod-autoscaler-vpa-recommender".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(2000)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(128)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(2)),
                            ),
                        },
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "vertical-pod-autoscaler-vpa-updater".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(2000)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(128)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                            ),
                        },
                    ],
                )),
                false => None,
            },
        })
    }
}

#[derive(Clone)]
struct VpaChartInstallationChecker {}

impl VpaChartInstallationChecker {
    pub fn new() -> Self {
        VpaChartInstallationChecker {}
    }
}

impl Default for VpaChartInstallationChecker {
    fn default() -> Self {
        VpaChartInstallationChecker::new()
    }
}

impl ChartInstallationChecker for VpaChartInstallationChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1368): Implement chart install verification
        // todo(pmavro): wait CRD to be ready
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::vertical_pod_autoscaler::VpaChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn vpa_chart_directory_exists_test() {
        // setup:
        let chart = VpaChart::new(
            None,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            false,
            HelmChartNamespaces::KubeSystem,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            VpaChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn vpa_chart_values_file_exists_test() {
        // setup:
        let chart = VpaChart::new(
            None,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            false,
            HelmChartNamespaces::KubeSystem,
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
                HelmChartType::Shared,
            ),
            VpaChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn vpa_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = VpaChart::new(
            None,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            crate::infrastructure::helm_charts::HelmChartResourcesConstraintType::ChartDefault,
            false,
            HelmChartNamespaces::KubeSystem,
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
                    HelmChartType::Shared,
                ),
                VpaChart::chart_name()
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
