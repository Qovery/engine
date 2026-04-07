use crate::infrastructure::models::kubernetes::KubernetesVersion;
use serde_derive::Serialize;

/// AWS EFS CSI addon
/// https://docs.aws.amazon.com/eks/latest/userguide/efs-csi.html
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsEfsCsiAddon {
    version: String,
}

impl AwsEfsCsiAddon {
    pub fn new_from_k8s_version(k8s_version: KubernetesVersion) -> Self {
        AwsEfsCsiAddon {
            // Get current default build of an aws-efs-csi add-on:
            // aws eks describe-addon-versions --kubernetes-version 1.30 --addon-name aws-efs-csi-driver | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                KubernetesVersion::V1_23 { .. } => "v2.1.6-eksbuild.1",
                KubernetesVersion::V1_24 { .. } => "v2.1.7-eksbuild.1",
                KubernetesVersion::V1_25 { .. } => "v2.1.7-eksbuild.1",
                KubernetesVersion::V1_26 { .. } => "v2.1.8-eksbuild.1",
                KubernetesVersion::V1_27 { .. } => "v2.1.14-eksbuild.1",
                KubernetesVersion::V1_28 { .. } => "v2.1.15-eksbuild.1",
                KubernetesVersion::V1_29 { .. } => "v2.3.1-eksbuild.1",
                KubernetesVersion::V1_30 { .. } => "v2.3.1-eksbuild.1",
                KubernetesVersion::V1_31 { .. } => "v2.3.1-eksbuild.1",
                KubernetesVersion::V1_32 { .. } => "v2.3.1-eksbuild.1",
                KubernetesVersion::V1_33 { .. } => "v2.3.1-eksbuild.1",
                KubernetesVersion::V1_34 { .. } => "v2.3.1-eksbuild.1",
            }
            .to_string(),
        }
    }

    pub fn new_with_overridden_version(addon_version: &str) -> Self {
        AwsEfsCsiAddon {
            version: addon_version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::models::kubernetes::KubernetesVersion;

    #[test]
    fn aws_addon_efs_csi_new_test() {
        // setup:
        struct TestCase {
            k8s_version: KubernetesVersion,
            expected: AwsEfsCsiAddon,
        }

        let tests_cases = vec![
            TestCase {
                k8s_version: KubernetesVersion::V1_23 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsEfsCsiAddon {
                    version: "v2.1.6-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_28 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsEfsCsiAddon {
                    version: "v2.1.15-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_33 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsEfsCsiAddon {
                    version: "v2.3.1-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_34 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsEfsCsiAddon {
                    version: "v2.3.1-eksbuild.1".to_string(),
                },
            },
        ];

        for tc in tests_cases {
            // execute:
            let result = AwsEfsCsiAddon::new_from_k8s_version(tc.k8s_version);

            // verify:
            assert_eq!(tc.expected, result);
        }
    }

    #[test]
    fn aws_addon_efs_csi_new_with_overridden_version() {
        // setup:
        let tests_cases = vec!["toto", "v2.1.6-eksbuild.1", "v2.3.1-eksbuild.1"];

        for tc in tests_cases {
            // execute:
            let result = AwsEfsCsiAddon::new_with_overridden_version(tc);

            // verify:
            assert_eq!(
                AwsEfsCsiAddon {
                    version: tc.to_string()
                },
                result
            );
        }
    }
}
