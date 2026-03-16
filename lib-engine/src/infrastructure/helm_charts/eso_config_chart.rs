use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::eso_chart::EsoClusterOutputs;
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::external_secrets::aws_secrets_manager_authentication::AwsAuthenticationMode;
use crate::infrastructure::models::external_secrets::gcp_secrets_manager_authentication::GcpAuthenticationMode;
use crate::infrastructure::models::external_secrets::{SecretsManagerAccess, SecretsManagerConnection};
use kube::Client;

pub struct EsoConfigChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    action: HelmAction,
    secrets_manager_accesses: Option<Vec<SecretsManagerAccess>>,
    eso_cluster_outputs: EsoClusterOutputs,
}

impl EsoConfigChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        namespace: HelmChartNamespaces,
        action: HelmAction,
        secrets_manager_accesses: Option<Vec<SecretsManagerAccess>>,
        eso_cluster_outputs: EsoClusterOutputs,
    ) -> Self {
        EsoConfigChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                EsoConfigChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                EsoConfigChart::chart_name(),
            ),
            namespace,
            action,
            secrets_manager_accesses,
            eso_cluster_outputs,
        }
    }

    pub fn chart_name() -> String {
        "external-secrets-config".to_string()
    }
}

impl ToCommonHelmChart for EsoConfigChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values = Vec::new();
        let mut values_json = Vec::new();

        // If secrets_manager_accesses is None, we don't generate any values
        // This will result in the chart deleting all ClusterSecretStores
        if let Some(accesses) = &self.secrets_manager_accesses {
            for (idx, access) in accesses.iter().enumerate() {
                match &access.connection {
                    SecretsManagerConnection::Aws(conn) => {
                        let region = &conn.region;
                        let authentication_mode = &conn.authentication_mode;
                        // Set common fields
                        values.push(ChartSetValue {
                            key: format!("authentications[{}].name", idx),
                            value: format!("store-aws-{}", access.id),
                        });
                        values.push(ChartSetValue {
                            key: format!("authentications[{}].secretManagerAccessId", idx),
                            value: access.id.clone(),
                        });
                        values.push(ChartSetValue {
                            key: format!("authentications[{}].region", idx),
                            value: region.clone(),
                        });

                        // Set authentication-specific fields
                        match authentication_mode {
                            AwsAuthenticationMode::Automatic => {
                                // Use the automatically generated role ARN from Terraform
                                let _role_arn = match &self.eso_cluster_outputs {
                                    EsoClusterOutputs::Aws {
                                        role_arn_automatically_generated: Some(arn),
                                    } => arn,
                                    EsoClusterOutputs::Aws {
                                        role_arn_automatically_generated: None,
                                    } => {
                                        return Err(HelmChartError::CommandError(CommandError::new_from_safe_message(
                                            "Automatic authentication mode requires role_arn_automatically_generated to be set".to_string(),
                                        )));
                                    }
                                    EsoClusterOutputs::Gcp { .. } => {
                                        return Err(HelmChartError::CommandError(CommandError::new_from_safe_message(
                                            "AWS authentication mode requires AWS cluster outputs".to_string(),
                                        )));
                                    }
                                };

                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].type", idx),
                                    value: "aws-iam".to_string(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].serviceAccount.name", idx),
                                    value: "external-secrets-operator-sa".to_string(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].serviceAccount.namespace", idx),
                                    value: "qovery".to_string(),
                                });
                            }
                            AwsAuthenticationMode::ArnRole { arn_role: _ } => {
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].type", idx),
                                    value: "aws-iam".to_string(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].serviceAccount.name", idx),
                                    value: "external-secrets-operator-sa".to_string(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].serviceAccount.namespace", idx),
                                    value: "qovery".to_string(),
                                });
                            }
                            AwsAuthenticationMode::AwsStaticCredentials {
                                access_key_id,
                                secret_access_key,
                            } => {
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].type", idx),
                                    value: "aws-static".to_string(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].accessKeyId", idx),
                                    value: access_key_id.clone(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].secretAccessKey", idx),
                                    value: secret_access_key.clone(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].secretName", idx),
                                    value: format!("{}-credentials", access.id),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].secretNamespace", idx),
                                    value: "qovery".to_string(),
                                });
                            }
                        }
                    }
                    SecretsManagerConnection::Gcp(conn) => {
                        let project_id = &conn.project_id;
                        let authentication_mode = &conn.authentication_mode;
                        // Set common fields
                        values.push(ChartSetValue {
                            key: format!("authentications[{}].name", idx),
                            value: format!("store-gcp-{}", access.id),
                        });
                        values.push(ChartSetValue {
                            key: format!("authentications[{}].secretManagerAccessId", idx),
                            value: access.id.clone(),
                        });
                        values.push(ChartSetValue {
                            key: format!("authentications[{}].projectId", idx),
                            value: project_id.clone(),
                        });

                        // Set authentication-specific fields
                        match authentication_mode {
                            GcpAuthenticationMode::Automatic => {
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].type", idx),
                                    value: "gcp-workload-identity".to_string(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].serviceAccount.name", idx),
                                    value: "external-secrets-operator-sa".to_string(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].serviceAccount.namespace", idx),
                                    value: "qovery".to_string(),
                                });
                            }
                            GcpAuthenticationMode::JsonCredentials { content } => {
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].type", idx),
                                    value: "gcp-static".to_string(),
                                });
                                values_json.push(ChartSetValue {
                                    key: format!("authentications[{}].jsonCredentials", idx),
                                    value: content.to_string(),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].secretName", idx),
                                    value: format!("{}-credentials", access.id),
                                });
                                values.push(ChartSetValue {
                                    key: format!("authentications[{}].secretNamespace", idx),
                                    value: "qovery".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: EsoConfigChart::chart_name(),
                action: self.action.clone(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values,
                values_json,
                ..Default::default()
            },
            chart_installation_checker: match self.action {
                HelmAction::Deploy => Some(Box::new(EsoConfigChartChecker::new())),
                HelmAction::Destroy => None,
            },
            vertical_pod_autoscaler: None,
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
pub struct EsoConfigChartChecker {}

impl EsoConfigChartChecker {
    pub fn new() -> Self {
        EsoConfigChartChecker {}
    }
}

impl Default for EsoConfigChartChecker {
    fn default() -> Self {
        EsoConfigChartChecker::new()
    }
}

impl ChartInstallationChecker for EsoConfigChartChecker {
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
    use crate::infrastructure::models::external_secrets::aws_secrets_manager_authentication::{
        AwsConnection, AwsSecretsManagerSource,
    };
    use crate::infrastructure::models::external_secrets::gcp_secrets_manager_authentication::GcpConnection;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn eso_config_chart_directory_exists_test() {
        // setup:
        let chart = EsoConfigChart::new(
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
            EsoConfigChart::chart_name(),
        );

        // execute
        let chart_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(chart_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn eso_config_chart_values_file_exists_test() {
        // setup:
        let chart = EsoConfigChart::new(
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
            EsoConfigChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&values_path);

        // verify:
        assert!(values_file.is_ok(), "Values file should exist: `{values_path}`");
    }

    /// Test generating values for AWS IAM role authentication
    #[test]
    fn test_aws_iam_authentication() {
        // setup:
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            Some(vec![SecretsManagerAccess {
                id: "test-iam".to_string(),
                connection: SecretsManagerConnection::Aws(AwsConnection {
                    source: AwsSecretsManagerSource::AwsSecretsManager,
                    region: "us-east-1".to_string(),
                    authentication_mode: AwsAuthenticationMode::ArnRole {
                        arn_role: "arn:aws:iam::123456789012:role/test-iam-role".to_string(),
                    },
                }),
            }]),
            EsoClusterOutputs::Aws {
                role_arn_automatically_generated: None,
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].name" && v.value == "store-aws-test-iam")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretManagerAccessId" && v.value == "test-iam")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].type" && v.value == "aws-iam")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].region" && v.value == "us-east-1")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].serviceAccount.name"
                    && v.value == "external-secrets-operator-sa")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].serviceAccount.namespace" && v.value == "qovery")
        );
    }

    /// Test generating values for AWS static credentials authentication
    #[test]
    fn test_aws_static_credentials_authentication() {
        // setup:
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            Some(vec![SecretsManagerAccess {
                id: "test-static".to_string(),
                connection: SecretsManagerConnection::Aws(AwsConnection {
                    source: AwsSecretsManagerSource::AwsSecretsManager,
                    region: "eu-west-3".to_string(),
                    authentication_mode: AwsAuthenticationMode::AwsStaticCredentials {
                        access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                        secret_access_key: "SECRET_KEYEMI/K7MDENG/bPxRfiCY".to_string(),
                    },
                }),
            }]),
            EsoClusterOutputs::Aws {
                role_arn_automatically_generated: None,
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].name" && v.value == "store-aws-test-static")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretManagerAccessId" && v.value == "test-static")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].type" && v.value == "aws-static")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].region" && v.value == "eu-west-3")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].accessKeyId" && v.value == "AKIAIOSFODNN7EXAMPLE")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretAccessKey" && v.value == "SECRET_KEYEMI/K7MDENG/bPxRfiCY")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretName" && v.value == "test-static-credentials")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretNamespace" && v.value == "qovery")
        );
    }

    /// Test generating values for automatic authentication mode
    #[test]
    fn test_automatic_authentication_with_generated_role() {
        // setup:
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            Some(vec![SecretsManagerAccess {
                id: "test-auto".to_string(),
                connection: SecretsManagerConnection::Aws(AwsConnection {
                    source: AwsSecretsManagerSource::AwsSecretsManager,
                    region: "ap-south-1".to_string(),
                    authentication_mode: AwsAuthenticationMode::Automatic,
                }),
            }]),
            EsoClusterOutputs::Aws {
                role_arn_automatically_generated: Some(
                    "arn:aws:iam::123456789012:role/auto-generated-role".to_string(),
                ),
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].name" && v.value == "store-aws-test-auto")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretManagerAccessId" && v.value == "test-auto")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].type" && v.value == "aws-iam")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].region" && v.value == "ap-south-1")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].serviceAccount.name"
                    && v.value == "external-secrets-operator-sa")
        );
    }

    /// Test that automatic authentication fails without generated role
    #[test]
    fn test_automatic_authentication_without_generated_role_fails() {
        // setup:
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            Some(vec![SecretsManagerAccess {
                id: "test-auto".to_string(),
                connection: SecretsManagerConnection::Aws(AwsConnection {
                    source: AwsSecretsManagerSource::AwsSecretsManager,
                    region: "us-east-1".to_string(),
                    authentication_mode: AwsAuthenticationMode::Automatic,
                }),
            }]),
            EsoClusterOutputs::Aws {
                role_arn_automatically_generated: None,
            },
        );

        // execute:
        let result = chart.to_common_helm_chart();

        // verify:
        assert!(result.is_err());
        if let Err(HelmChartError::CommandError(err)) = result {
            assert!(err.message_safe().contains("role_arn_automatically_generated"));
        } else {
            panic!("Expected CommandError");
        }
    }

    /// Test multiple authentications
    #[test]
    fn test_multiple_authentications() {
        // setup:
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            Some(vec![
                SecretsManagerAccess {
                    id: "first-iam".to_string(),
                    connection: SecretsManagerConnection::Aws(AwsConnection {
                        source: AwsSecretsManagerSource::AwsSecretsManager,
                        region: "us-east-1".to_string(),
                        authentication_mode: AwsAuthenticationMode::ArnRole {
                            arn_role: "arn:aws:iam::123456789012:role/first-iam-role".to_string(),
                        },
                    }),
                },
                SecretsManagerAccess {
                    id: "second-static".to_string(),
                    connection: SecretsManagerConnection::Aws(AwsConnection {
                        source: AwsSecretsManagerSource::AwsSecretsManager,
                        region: "us-east-1".to_string(),
                        authentication_mode: AwsAuthenticationMode::AwsStaticCredentials {
                            access_key_id: "AKIAEXAMPLE1".to_string(),
                            secret_access_key: "SECRET1".to_string(),
                        },
                    }),
                },
            ]),
            EsoClusterOutputs::Aws {
                role_arn_automatically_generated: None,
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;

        // Check first authentication
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].name" && v.value == "store-aws-first-iam")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].type" && v.value == "aws-iam")
        );

        // Check second authentication
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[1].name" && v.value == "store-aws-second-static")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[1].type" && v.value == "aws-static")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[1].accessKeyId" && v.value == "AKIAEXAMPLE1")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[1].secretAccessKey" && v.value == "SECRET1")
        );
    }

    /// Test GCP Workload Identity authentication
    #[test]
    fn test_gcp_workload_identity_authentication() {
        // setup:
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            Some(vec![SecretsManagerAccess {
                id: "gcp-workload".to_string(),
                connection: SecretsManagerConnection::Gcp(GcpConnection {
                    region: "us-central1".to_string(),
                    project_id: "my-gcp-project-123456".to_string(),
                    authentication_mode: GcpAuthenticationMode::Automatic,
                }),
            }]),
            EsoClusterOutputs::Gcp {
                eso_operator_service_account_email: None,
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].name" && v.value == "store-gcp-gcp-workload")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretManagerAccessId" && v.value == "gcp-workload")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].type" && v.value == "gcp-workload-identity")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].projectId" && v.value == "my-gcp-project-123456")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].serviceAccount.name"
                    && v.value == "external-secrets-operator-sa")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].serviceAccount.namespace" && v.value == "qovery")
        );
    }

    /// Test GCP JSON Credentials authentication
    #[test]
    fn test_gcp_json_credentials_authentication() {
        // setup:
        let json_creds = r#"{"type":"service_account","project_id":"test-project","private_key":"key"}"#;
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            Some(vec![SecretsManagerAccess {
                id: "gcp-static".to_string(),
                connection: SecretsManagerConnection::Gcp(GcpConnection {
                    region: "us-central1".to_string(),
                    project_id: "my-gcp-project-789012".to_string(),
                    authentication_mode: GcpAuthenticationMode::JsonCredentials {
                        content: json_creds.to_string(),
                    },
                }),
            }]),
            EsoClusterOutputs::Gcp {
                eso_operator_service_account_email: None,
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;
        let values_json = &common_chart.chart_info.values_json;
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].name" && v.value == "store-gcp-gcp-static")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretManagerAccessId" && v.value == "gcp-static")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].type" && v.value == "gcp-static")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].projectId" && v.value == "my-gcp-project-789012")
        );
        assert!(
            values_json
                .iter()
                .any(|v| v.key == "authentications[0].jsonCredentials" && v.value == json_creds)
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretName" && v.value == "gcp-static-credentials")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].secretNamespace" && v.value == "qovery")
        );
    }

    /// Test mixed AWS and GCP authentications
    #[test]
    fn test_mixed_aws_and_gcp_authentications() {
        // setup:
        let accesses = Some(vec![
            SecretsManagerAccess {
                id: "aws-access".to_string(),
                connection: SecretsManagerConnection::Aws(AwsConnection {
                    source: AwsSecretsManagerSource::AwsSecretsManager,
                    region: "us-east-1".to_string(),
                    authentication_mode: AwsAuthenticationMode::AwsStaticCredentials {
                        access_key_id: "AKIAEXAMPLE".to_string(),
                        secret_access_key: "SECRET".to_string(),
                    },
                }),
            },
            SecretsManagerAccess {
                id: "gcp-access".to_string(),
                connection: SecretsManagerConnection::Gcp(GcpConnection {
                    region: "us-central1".to_string(),
                    project_id: "my-project".to_string(),
                    authentication_mode: GcpAuthenticationMode::Automatic,
                }),
            },
        ]);

        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            accesses,
            EsoClusterOutputs::Gcp {
                eso_operator_service_account_email: Some("eso@operator.com".to_string()),
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;

        // Check AWS authentication
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].name" && v.value == "store-aws-aws-access")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[0].type" && v.value == "aws-static")
        );

        // Check GCP authentication
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[1].name" && v.value == "store-gcp-gcp-access")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[1].type" && v.value == "gcp-workload-identity")
        );
        assert!(
            values
                .iter()
                .any(|v| v.key == "authentications[1].projectId" && v.value == "my-project")
        );
    }

    /// Test that None secrets_manager_accesses results in no values (deletes all ClusterSecretStores)
    #[test]
    fn test_none_accesses_generates_no_values() {
        // setup:
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            None,
            EsoClusterOutputs::Gcp {
                eso_operator_service_account_email: Some("eso@operator.com".to_string()),
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;
        // No authentication values should be generated
        assert!(values.is_empty());
    }

    /// Test that empty vector results in no values (deletes all ClusterSecretStores)
    #[test]
    fn test_empty_accesses_generates_no_values() {
        // setup:
        let chart = EsoConfigChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmAction::Deploy,
            Some(vec![]),
            EsoClusterOutputs::Gcp {
                eso_operator_service_account_email: Some("eso@operator.com".to_string()),
            },
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify:
        let values = &common_chart.chart_info.values;
        // No authentication values should be generated
        assert!(values.is_empty());
    }
}
