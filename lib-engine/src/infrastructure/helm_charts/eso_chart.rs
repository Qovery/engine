use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::external_secrets::aws_secrets_manager_authentication::AwsAuthenticationMode;
use crate::infrastructure::models::external_secrets::{SecretsManagerAccess, SecretsManagerConnection};
use kube::Client;

pub struct EsoChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    action: HelmAction,
    secrets_manager_accesses: Option<Vec<SecretsManagerAccess>>,
    eso_cluster_outputs: EsoClusterOutputs,
}

#[derive(Clone)]
pub enum EsoClusterOutputs {
    Aws {
        role_arn_automatically_generated: Option<String>,
    },
    Gcp {
        eso_operator_service_account_email: Option<String>,
    },
}

impl EsoChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        namespace: HelmChartNamespaces,
        action: HelmAction,
        secrets_manager_accesses: Option<Vec<SecretsManagerAccess>>,
        eso_cluster_outputs: EsoClusterOutputs,
    ) -> Self {
        EsoChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                EsoChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                EsoChart::chart_name(),
            ),
            namespace,
            action,
            secrets_manager_accesses,
            eso_cluster_outputs,
        }
    }

    pub fn chart_name() -> String {
        "external-secrets".to_string()
    }
}

impl ToCommonHelmChart for EsoChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        fn generate_eks_values(
            secrets_manager_accesses: &[SecretsManagerAccess],
            role_arn_automatically_generated: Option<&String>,
        ) -> Vec<ChartSetValue> {
            // If the role has been automatically created, take it
            if let Some(role_arn_automatically_generated) = role_arn_automatically_generated {
                return vec![
                    ChartSetValue {
                        key: "serviceAccount.create".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "serviceAccount.name".to_string(),
                        value: "external-secrets-operator-sa".to_string(),
                    },
                    ChartSetValue {
                        key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: role_arn_automatically_generated.to_string(),
                    },
                ];
            }

            // Otherwise, look for ARN role authentication
            let provided_role_arn = secrets_manager_accesses
                .iter()
                .find_map(|access| match &access.connection {
                    SecretsManagerConnection::Aws(conn) => match &conn.authentication_mode {
                        AwsAuthenticationMode::ArnRole { arn_role } => Some(arn_role.clone()),
                        _ => None,
                    },
                    _ => None,
                });

            if let Some(role_arn) = provided_role_arn {
                return vec![
                    ChartSetValue {
                        key: "serviceAccount.create".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "serviceAccount.name".to_string(),
                        value: "external-secrets-operator-sa".to_string(),
                    },
                    ChartSetValue {
                        key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: role_arn,
                    },
                ];
            }

            // Else it means static creds are used so nothing to do here
            vec![
                ChartSetValue {
                    key: "serviceAccount.create".to_string(),
                    value: "false".to_string(),
                },
                ChartSetValue {
                    key: "serviceAccount.name".to_string(),
                    value: "".to_string(),
                },
            ]
        }

        fn generate_gke_values(eso_operator_service_account_email: Option<&String>) -> Vec<ChartSetValue> {
            // If the role has been automatically created, take it
            if let Some(operator_service_account_email) = eso_operator_service_account_email {
                return vec![
                    ChartSetValue {
                        key: "serviceAccount.create".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "serviceAccount.name".to_string(),
                        value: "external-secrets-operator-sa".to_string(),
                    },
                    ChartSetValue {
                        key: r"serviceAccount.annotations.iam\.gke\.io/gcp-service-account".to_string(),
                        value: operator_service_account_email.to_string(),
                    },
                ];
            }

            // Else it means static creds are used so nothing to do here
            vec![
                ChartSetValue {
                    key: "serviceAccount.create".to_string(),
                    value: "false".to_string(),
                },
                ChartSetValue {
                    key: "serviceAccount.name".to_string(),
                    value: "".to_string(),
                },
            ]
        }

        let values = match self.secrets_manager_accesses.as_ref() {
            None => vec![],
            Some(accesses) => match &self.eso_cluster_outputs {
                EsoClusterOutputs::Aws {
                    role_arn_automatically_generated,
                } => generate_eks_values(accesses, role_arn_automatically_generated.as_ref()),
                EsoClusterOutputs::Gcp {
                    eso_operator_service_account_email,
                } => generate_gke_values(eso_operator_service_account_email.as_ref()),
            },
        };

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: EsoChart::chart_name(),
                action: self.action.clone(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values,
                ..Default::default()
            },
            chart_installation_checker: match self.action {
                HelmAction::Deploy => Some(Box::new(EsoChartChecker::new())),
                HelmAction::Destroy => None,
            },
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct EsoChartChecker {}

impl EsoChartChecker {
    pub fn new() -> Self {
        EsoChartChecker {}
    }
}

impl Default for EsoChartChecker {
    fn default() -> Self {
        EsoChartChecker::new()
    }
}

impl ChartInstallationChecker for EsoChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::helm_charts::{HelmChartType, get_helm_path_kubernetes_provider_sub_folder_name};
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn eso_chart_directory_exists_test() {
        // setup:
        let chart = EsoChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            None,
            EsoClusterOutputs::Aws {
                role_arn_automatically_generated: None,
            },
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            EsoChart::chart_name(),
        );

        // execute
        let chart_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(chart_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn eso_chart_values_file_exists_test() {
        // setup:
        let chart = EsoChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            None,
            EsoClusterOutputs::Aws {
                role_arn_automatically_generated: None,
            },
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared
            ),
            EsoChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&values_path);

        // verify:
        assert!(values_file.is_ok(), "Values file should exist: `{values_path}`");
    }
}
