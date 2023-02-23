use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, CommonChart};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use crate::runtime::block_on;
use k8s_openapi::api::storage::v1::StorageClass;
use kube::core::params::ListParams;
use kube::Api;
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
    _chart_values_path: HelmChartValuesFilePath, // TODO(benjamin): to be used iinstead of having chart duplicated per cloud providers
    storage_types_to_be_checked_after_install: HashSet<QoveryStorageType>,
}

impl QoveryStorageClassChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        storage_types_to_be_checked_after_install: HashSet<QoveryStorageType>,
    ) -> Self {
        QoveryStorageClassChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                QoveryStorageClassChart::chart_name(),
            ),
            _chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                QoveryStorageClassChart::chart_name(),
            ),
            storage_types_to_be_checked_after_install,
        }
    }

    pub fn chart_name() -> String {
        "q-storageclass".to_string()
    }
}

impl ToCommonHelmChart for QoveryStorageClassChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: QoveryStorageClassChart::chart_name(),
                path: self.chart_path.to_string(),
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryStorageClassChartInstallationChecker::new(
                self.storage_types_to_be_checked_after_install.clone(),
            ))),
        }
    }
}

#[derive(Clone)]
pub struct QoveryStorageClassChartInstallationChecker {
    storage_types_to_be_checked_after_install: HashSet<QoveryStorageType>,
}

impl QoveryStorageClassChartInstallationChecker {
    pub fn new(storage_types_to_be_checked_after_install: HashSet<QoveryStorageType>) -> Self {
        QoveryStorageClassChartInstallationChecker {
            storage_types_to_be_checked_after_install,
        }
    }
}

impl ChartInstallationChecker for QoveryStorageClassChartInstallationChecker {
    fn verify_installation(&self, kube_client: &kube::Client) -> Result<(), CommandError> {
        let storage_classes: Api<StorageClass> = Api::all(kube_client.clone());

        // Check all Qovery's required storage classes are properly set
        for required_storage_class in self.storage_types_to_be_checked_after_install.iter() {
            match block_on(
                storage_classes
                    .list(&ListParams::default().labels(format!("qovery-type={required_storage_class}").as_str())),
            ) {
                Ok(storage_classes_result) => {
                    if storage_classes_result.items.is_empty() {
                        return Err(CommandError::new_from_safe_message(format!(
                            "Error: q-storage-class (qovery-type={required_storage_class}) is not set"
                        )));
                    }
                }
                Err(e) => {
                    return Err(CommandError::new(
                        format!("Error trying to get q-storage-class (qovery-type={required_storage_class})"),
                        Some(e.to_string()),
                        None,
                    ))
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
    use crate::cloud_provider::helm_charts::qovery_storage_class_chart::QoveryStorageClassChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use std::collections::HashSet;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_storage_class_chart_directory_exists_test() {
        // setup:
        let chart = QoveryStorageClassChart::new(None, HashSet::new());

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            QoveryStorageClassChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    #[ignore] // TODO(benjaminch): To be activated once moved to values file
    fn qovery_storage_class_chart_values_file_exists_test() {
        // setup:
        let chart = QoveryStorageClassChart::new(None, HashSet::new());

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart._chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
            ),
            QoveryStorageClassChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    #[ignore] // TODO(benjaminch): To be activated once moved to values file
    fn qovery_storage_class_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = QoveryStorageClassChart::new(None, HashSet::new());
        let common_chart = chart.to_common_helm_chart();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart._chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
                ),
                QoveryStorageClassChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
