use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, CommonChart};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
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

pub struct QoveryStorageClassChart {
    chart_path: HelmChartPath,
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
            storage_types_to_be_checked_after_install,
        }
    }

    fn chart_name() -> String {
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
                    .list(&ListParams::default().labels(format!("qovery-type={}", required_storage_class).as_str())),
            ) {
                Ok(storage_classes_result) => {
                    if storage_classes_result.items.is_empty() {
                        return Err(CommandError::new_from_safe_message(format!(
                            "Error: q-storage-class (qovery-type={}) is not set",
                            required_storage_class
                        )));
                    }
                }
                Err(e) => {
                    return Err(CommandError::new(
                        format!("Error trying to get q-storage-class (qovery-type={})", required_storage_class),
                        Some(e.to_string()),
                        None,
                    ))
                }
            }
        }

        Ok(())
    }
}
