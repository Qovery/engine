use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::cloud_provider::Kind;
use crate::io_models::models::StorageClass as StorageClassModel;
use crate::runtime::block_on;
use k8s_openapi::api::storage::v1::StorageClass;
use kube::Api;
use kube::core::params::ListParams;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum QoveryStorageType {
    Ssd,
    Hdd,
    Cold,
    Nvme,
}

impl Display for QoveryStorageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            QoveryStorageType::Ssd => "ssd",
            QoveryStorageType::Hdd => "hdd",
            QoveryStorageType::Cold => "cold",
            QoveryStorageType::Nvme => "nvme",
        })
    }
}

// TODO(benjaminch): properly refactor this chart, should be common and handled per cloud providers via values files.
pub struct QoveryStorageClassChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    default_storage_class: Option<StorageClassModel>,
    storage_types_to_be_checked_after_install: HashSet<QoveryStorageType>,
}

impl QoveryStorageClassChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        cloud_provider: Kind,
        storage_types_to_be_checked_after_install: HashSet<QoveryStorageType>,
        namespace: HelmChartNamespaces,
        default_storage_class: Option<StorageClassModel>,
    ) -> Self {
        QoveryStorageClassChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                QoveryStorageClassChart::chart_name_with_cloud_provider_name(cloud_provider.clone()),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                QoveryStorageClassChart::chart_name(),
            ),
            namespace,
            default_storage_class,
            storage_types_to_be_checked_after_install,
        }
    }

    pub fn chart_name() -> String {
        "q-storageclass".to_string()
    }

    pub fn chart_name_with_cloud_provider_name(cloud_provider: Kind) -> String {
        format!("q-storageclass-{}", cloud_provider.to_string().to_lowercase())
    }
}

impl ToCommonHelmChart for QoveryStorageClassChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut chart_set_values = vec![];
        if let Some(default_storage_class) = &self.default_storage_class {
            chart_set_values.push(ChartSetValue {
                key: "defaultStorageClassName".to_string(),
                value: default_storage_class.to_string(),
            });
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryStorageClassChart::chart_name(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: chart_set_values,
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryStorageClassChartInstallationChecker::new(
                self.storage_types_to_be_checked_after_install.clone(),
                self.default_storage_class.clone(),
            ))),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct QoveryStorageClassChartInstallationChecker {
    storage_types_to_be_checked_after_install: HashSet<QoveryStorageType>,
    _default_storage_class: Option<StorageClassModel>,
}

impl QoveryStorageClassChartInstallationChecker {
    pub fn new(
        storage_types_to_be_checked_after_install: HashSet<QoveryStorageType>,
        default_storage_class: Option<StorageClassModel>,
    ) -> Self {
        QoveryStorageClassChartInstallationChecker {
            storage_types_to_be_checked_after_install,
            _default_storage_class: default_storage_class,
        }
    }
}

impl ChartInstallationChecker for QoveryStorageClassChartInstallationChecker {
    fn verify_installation(&self, kube_client: &kube::Client) -> Result<(), CommandError> {
        let storage_classes: Api<StorageClass> = Api::all(kube_client.clone());

        for required_storage_class in &self.storage_types_to_be_checked_after_install {
            let storage_classes_result = block_on(
                storage_classes.list(&ListParams::default().labels(&format!("qovery-type={required_storage_class}"))),
            )
            .map_err(|e| {
                CommandError::new(
                    format!("Error trying to get q-storage-class (qovery-type={required_storage_class})"),
                    Some(e.to_string()),
                    None,
                )
            })?;

            if storage_classes_result.items.is_empty() {
                return Err(CommandError::new_from_safe_message(format!(
                    "Error: q-storage-class (qovery-type={required_storage_class}) is not set"
                )));
            }
        }

        // TODO(benjaminch): reactivate this check once it works properly
        // if let Some(default_storage_class) = &self.default_storage_class {
        //     // check if default storage class is set (if provided)
        //     let storage_classes_result = block_on(
        //         storage_classes.list(&ListParams::default().fields(&format!("metadata.name={default_storage_class}"))),
        //     )
        //     .map_err(|e| {
        //         CommandError::new(
        //             format!("Error trying to get default storage-class (name={default_storage_class})"),
        //             Some(e.to_string()),
        //             None,
        //         )
        //     })?;

        //     let is_default_storage_class_set = storage_classes_result.items.iter().any(|sc| {
        //         sc.metadata.name == Some(default_storage_class.to_string())
        //             && sc
        //                 .metadata
        //                 .annotations
        //                 .as_ref()
        //                 .and_then(|annotations| annotations.get("storageclass.kubernetes.io/is-default-class"))
        //                 .map_or(false, |value| value == "true")
        //     });

        //     if !is_default_storage_class_set {
        //         return Err(CommandError::new_from_safe_message(format!(
        //             "Error: storage-class ({default_storage_class}) is not set as default"
        //         )));
        //     }
        // }

        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::qovery_storage_class_chart::QoveryStorageClassChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::cloud_provider::Kind;
    use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
    use std::collections::HashSet;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_storage_class_chart_directory_exists_test() {
        for cloud_provider_kind in [Kind::Aws, Kind::Gcp, Kind::Scw] {
            // setup:
            let chart = QoveryStorageClassChart::new(
                None,
                cloud_provider_kind.clone(),
                HashSet::new(),
                HelmChartNamespaces::KubeSystem,
                None,
            );

            let current_directory = env::current_dir().expect("Impossible to get current directory");
            let chart_path = format!(
                "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
                current_directory
                    .to_str()
                    .expect("Impossible to convert current directory to string"),
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(match cloud_provider_kind {
                        Kind::Aws => KubernetesKind::Eks,
                        Kind::Gcp => KubernetesKind::Gke,
                        Kind::Scw => KubernetesKind::ScwKapsule,
                        _ => unreachable!(),
                    }),
                ),
                QoveryStorageClassChart::chart_name_with_cloud_provider_name(cloud_provider_kind),
            );

            // execute
            let values_file = std::fs::File::open(&chart_path);

            // verify:
            assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
        }
    }

    /// Makes sure chart values file exists.
    #[test]
    fn qovery_storage_class_chart_values_file_exists_test() {
        for cloud_provider_kind in [Kind::Aws, Kind::Gcp, Kind::Scw] {
            // setup:
            let chart = QoveryStorageClassChart::new(
                None,
                cloud_provider_kind.clone(),
                HashSet::new(),
                HelmChartNamespaces::KubeSystem,
                None,
            );

            let current_directory = env::current_dir().expect("Impossible to get current directory");
            let chart_values_path = format!(
                "{}/lib/{}/bootstrap/chart_values/{}.yaml",
                current_directory
                    .to_str()
                    .expect("Impossible to convert current directory to string"),
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(match cloud_provider_kind {
                        Kind::Aws => KubernetesKind::Eks,
                        Kind::Gcp => KubernetesKind::Gke,
                        Kind::Scw => KubernetesKind::ScwKapsule,
                        _ => unreachable!(),
                    }),
                ),
                QoveryStorageClassChart::chart_name(),
            );

            // execute
            let values_file = std::fs::File::open(&chart_values_path);

            // verify:
            assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
        }
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn qovery_storage_class_chart_rust_overridden_values_exists_in_values_yaml_test() {
        for cloud_provider_kind in [Kind::Aws, Kind::Gcp, Kind::Scw] {
            // setup:
            let chart = QoveryStorageClassChart::new(
                None,
                cloud_provider_kind.clone(),
                HashSet::new(),
                HelmChartNamespaces::KubeSystem,
                None,
            );
            let common_chart = chart.to_common_helm_chart().unwrap();

            // execute:
            let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
                common_chart,
                format!(
                    "/lib/{}/bootstrap/chart_values/{}.yaml",
                    get_helm_path_kubernetes_provider_sub_folder_name(
                        chart.chart_values_path.helm_path(),
                        HelmChartType::CloudProviderSpecific(match cloud_provider_kind {
                            Kind::Aws => KubernetesKind::Eks,
                            Kind::Gcp => KubernetesKind::Gke,
                            Kind::Scw => KubernetesKind::ScwKapsule,
                            _ => unreachable!(),
                        }),
                    ),
                    QoveryStorageClassChart::chart_name(),
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
}
