use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use crate::runtime::block_on;
use k8s_openapi::api::rbac::v1::RoleBinding;
use kube::core::params::ListParams;
use kube::{Api, Client};

pub struct AwsIamEksUserMapperChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    chart_image_region: String,
    aws_iam_eks_user_mapper_key: String,
    aws_iam_eks_user_mapper_secret: String,
    aws_iam_user_mapper_group_name: String,
}

impl AwsIamEksUserMapperChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_image_region: String,
        aws_iam_eks_user_mapper_key: String,
        aws_iam_eks_user_mapper_secret: String,
        aws_iam_user_mapper_group_name: String,
    ) -> AwsIamEksUserMapperChart {
        AwsIamEksUserMapperChart {
            chart_image_region,
            aws_iam_eks_user_mapper_key,
            aws_iam_eks_user_mapper_secret,
            aws_iam_user_mapper_group_name,
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                AwsIamEksUserMapperChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                AwsIamEksUserMapperChart::chart_name(),
            ),
        }
    }

    pub fn chart_name() -> String {
        "iam-eks-user-mapper".to_string()
    }
}

impl ToCommonHelmChart for AwsIamEksUserMapperChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: AwsIamEksUserMapperChart::chart_name(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "aws.accessKey".to_string(),
                        value: self.aws_iam_eks_user_mapper_key.to_string(),
                    },
                    ChartSetValue {
                        key: "aws.secretKey".to_string(),
                        value: self.aws_iam_eks_user_mapper_secret.to_string(),
                    },
                    ChartSetValue {
                        key: "aws.region".to_string(),
                        value: self.chart_image_region.to_string(),
                    },
                    ChartSetValue {
                        key: "syncIamGroup".to_string(),
                        value: self.aws_iam_user_mapper_group_name.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(AwsIamEksUserMapperChecker::new())),
        }
    }
}

pub struct AwsIamEksUserMapperChecker {}

impl AwsIamEksUserMapperChecker {
    pub fn new() -> Self {
        AwsIamEksUserMapperChecker {}
    }
}

impl Default for AwsIamEksUserMapperChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartInstallationChecker for AwsIamEksUserMapperChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        // Check user 'iam-eks-user-mapper' is properly set
        let iam_user_mapper_role: Api<RoleBinding> = Api::all(kube_client.clone());

        match block_on(
            iam_user_mapper_role
                .list(&ListParams::default().fields("metadata.name=eks-configmap-modifier-rolebinding")),
        ) {
            Ok(iam_user_mapper_role_result) => {
                if iam_user_mapper_role_result.items.is_empty() {
                    return Err(CommandError::new_from_safe_message(
                        format!("Required role binding `eks-configmap-modifier-role` created by `{}` chart not found, chart is not installed properly.", AwsIamEksUserMapperChart::chart_name()),
                    ));
                }

                for role_binding in iam_user_mapper_role_result.items {
                    // Check if it references the proper role
                    if role_binding.role_ref.name.to_lowercase() != "eks-configmap-modifier-role" {
                        return Err(CommandError::new_from_safe_message(
                            format!("Role binding `eks-configmap-modifier-rolebinding` created by `{}` chart, not installed properly: it should references `eks-configmap-modifier-role` role.", AwsIamEksUserMapperChart::chart_name()),
                        ));
                    }

                    // Check if contains the subject
                    if let Some(subjects) = role_binding.subjects {
                        if !subjects.iter().any(|e| {
                            e.name.to_lowercase() == "iam-eks-user-mapper" && e.kind.to_lowercase() == "serviceaccount"
                        }) {
                            return Err(CommandError::new_from_safe_message(
                                format!("Role binding `eks-configmap-modifier-rolebinding` created by `{}` chart, not installed properly: it should have `iam-eks-user-mapper` subject.", AwsIamEksUserMapperChart::chart_name()),
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                return Err(CommandError::new(
                    "Error trying to get role binding `eks-configmap-modifier-role`".to_string(),
                    Some(e.to_string()),
                    None,
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::helm_charts::aws_iam_eks_user_mapper_chart::AwsIamEksUserMapperChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn aws_iam_eks_user_mapper_chart_directory_exists_test() {
        // setup:
        let chart = AwsIamEksUserMapperChart::new(
            None,
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
        );

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
            AwsIamEksUserMapperChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn aws_iam_eks_user_mapper_chart_values_file_exists_test() {
        // setup:
        let chart = AwsIamEksUserMapperChart::new(
            None,
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            AwsIamEksUserMapperChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn aws_iam_eks_user_mapper_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = AwsIamEksUserMapperChart::new(
            None,
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
        );
        let common_chart = chart.to_common_helm_chart();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
                ),
                AwsIamEksUserMapperChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
