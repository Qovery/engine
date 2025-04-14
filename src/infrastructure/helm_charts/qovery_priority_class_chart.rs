use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces,
    QoveryPriorityClass,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::runtime::block_on;
use k8s_openapi::api::scheduling::v1::PriorityClass;
use kube::Api;
use kube::core::params::ListParams;
use kube::core::{Expression, Selector};
use std::collections::HashSet;

pub struct QoveryPriorityClassChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    priority_classes_to_be_installed: HashSet<QoveryPriorityClass>,
}

impl QoveryPriorityClassChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        priority_classes_to_be_checked_after_install: HashSet<QoveryPriorityClass>,
        namespace: HelmChartNamespaces,
    ) -> Self {
        QoveryPriorityClassChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryPriorityClassChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryPriorityClassChart::chart_name(),
            ),
            namespace,
            priority_classes_to_be_installed: priority_classes_to_be_checked_after_install,
        }
    }

    pub fn chart_name() -> String {
        "qovery-priority-class".to_string()
    }
}

impl ToCommonHelmChart for QoveryPriorityClassChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryPriorityClassChart::chart_name(),
                namespace: self.namespace,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "priorityClass.highPriority.enable".to_string(),
                        value: self
                            .priority_classes_to_be_installed
                            .contains(&QoveryPriorityClass::HighPriority)
                            .to_string(),
                    },
                    ChartSetValue {
                        key: "priorityClass.standardPriority.enable".to_string(),
                        value: self
                            .priority_classes_to_be_installed
                            .contains(&QoveryPriorityClass::StandardPriority)
                            .to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryPriorityClassChartInstallationChecker::new(
                self.priority_classes_to_be_installed.clone(),
            ))),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct QoveryPriorityClassChartInstallationChecker {
    priority_classes_to_be_checked_after_install: HashSet<QoveryPriorityClass>,
}

impl QoveryPriorityClassChartInstallationChecker {
    pub fn new(priority_classes_to_be_checked_after_install: HashSet<QoveryPriorityClass>) -> Self {
        QoveryPriorityClassChartInstallationChecker {
            priority_classes_to_be_checked_after_install,
        }
    }
}

impl ChartInstallationChecker for QoveryPriorityClassChartInstallationChecker {
    fn verify_installation(&self, kube_client: &kube::Client) -> Result<(), CommandError> {
        let priority_classes: Api<PriorityClass> = Api::all(kube_client.clone());

        if !self.priority_classes_to_be_checked_after_install.is_empty() {
            let selector: Selector = Expression::In(
                "qovery-type".to_string(),
                self.priority_classes_to_be_checked_after_install
                    .iter()
                    .map(|pc| pc.to_string().to_lowercase())
                    .collect(),
            )
            .into();

            match block_on(priority_classes.list(&ListParams::default().labels_from(&selector))) {
                Ok(priority_classes_result) => {
                    let installed_priority_classes: HashSet<String, std::collections::hash_map::RandomState> =
                        HashSet::from_iter(
                            priority_classes_result
                                .items
                                .into_iter()
                                .filter_map(|item| item.metadata.name.map(|name| name.to_lowercase())),
                        );
                    for required_priority_class in self.priority_classes_to_be_checked_after_install.iter() {
                        if !installed_priority_classes.contains(&required_priority_class.to_string().to_lowercase()) {
                            return Err(CommandError::new_from_safe_message(format!(
                                "Error: q-priority-class (metadata.name={required_priority_class}) is not set"
                            )));
                        }
                    }
                }
                Err(e) => {
                    return Err(CommandError::new(
                        format!("Error trying to get q-priority-class ({selector})",),
                        Some(e.to_string()),
                        None,
                    ));
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
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::qovery_priority_class_chart::QoveryPriorityClassChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use std::collections::HashSet;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_priority_class_chart_directory_exists_test() {
        // setup:
        let chart = QoveryPriorityClassChart::new(None, HashSet::new(), HelmChartNamespaces::KubeSystem);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            QoveryPriorityClassChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn qovery_priority_class_chart_values_file_exists_test() {
        // setup:
        let chart = QoveryPriorityClassChart::new(None, HashSet::new(), HelmChartNamespaces::KubeSystem);

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
            QoveryPriorityClassChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn qovery_priority_class_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = QoveryPriorityClassChart::new(None, HashSet::new(), HelmChartNamespaces::KubeSystem);
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
                QoveryPriorityClassChart::chart_name()
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
