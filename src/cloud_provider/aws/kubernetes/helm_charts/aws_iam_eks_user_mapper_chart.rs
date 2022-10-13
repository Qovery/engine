use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use crate::runtime::block_on;
use k8s_openapi::api::rbac::v1::RoleBinding;
use kube::core::params::ListParams;
use kube::{Api, Client};

pub struct AwsIamEksUserMapperChart {
    chart_path: HelmChartPath,
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
        }
    }

    fn chart_name() -> String {
        "iam-eks-user-mapper".to_string()
    }
}

impl ToCommonHelmChart for AwsIamEksUserMapperChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: AwsIamEksUserMapperChart::chart_name(),
                path: self.chart_path.to_string(),
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
                    // resources limits
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: "20m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: "10m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: "32Mi".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: "32Mi".to_string(),
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
