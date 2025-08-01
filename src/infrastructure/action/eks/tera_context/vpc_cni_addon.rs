use crate::infrastructure::models::kubernetes::KubernetesVersion;
use serde_derive::Serialize;

/// AWS VPC CNI addon
/// https://docs.aws.amazon.com/fr_fr/eks/latest/userguide/managing-vpc-cni.html
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsVpcCniAddon {
    version: String,
}

impl AwsVpcCniAddon {
    pub fn new_from_k8s_version(k8s_version: KubernetesVersion) -> Self {
        AwsVpcCniAddon {
            // Get current default build of an aws-cni add-on:
            // https://docs.aws.amazon.com/eks/latest/userguide/managing-vpc-cni.html OR
            // aws eks describe-addon-versions --kubernetes-version 1.23 --addon-name vpc-cni | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                KubernetesVersion::V1_23 { .. } => "v1.12.1-eksbuild.1",
                KubernetesVersion::V1_24 { .. } => "v1.12.2-eksbuild.1",
                KubernetesVersion::V1_25 { .. } => "v1.13.2-eksbuild.1",
                KubernetesVersion::V1_26 { .. } => "v1.13.2-eksbuild.1",
                KubernetesVersion::V1_27 { .. } => "v1.15.1-eksbuild.1",
                KubernetesVersion::V1_28 { .. } => "v1.18.0-eksbuild.1",
                KubernetesVersion::V1_29 { .. } => "v1.18.3-eksbuild.2",
                KubernetesVersion::V1_30 { .. } => "v1.18.3-eksbuild.2",
                KubernetesVersion::V1_31 { .. } => "v1.19.3-eksbuild.1",
                KubernetesVersion::V1_32 { .. } => "v1.19.6-eksbuild.7",
                KubernetesVersion::V1_33 { .. } => "v1.19.6-eksbuild.7",
            }
            .to_string(),
        }
    }

    pub fn new_with_overridden_version(addon_version: &str) -> Self {
        AwsVpcCniAddon {
            version: addon_version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::models::kubernetes::KubernetesVersion;

    #[test]
    fn aws_addon_cni_new_test() {
        // setup:
        struct TestCase {
            k8s_version: KubernetesVersion,
            expected: AwsVpcCniAddon,
        }

        let tests_cases = vec![
            TestCase {
                k8s_version: KubernetesVersion::V1_23 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.12.1-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_24 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.12.2-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_25 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.13.2-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_26 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.13.2-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_27 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.15.1-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_28 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.18.0-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_29 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.18.3-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_30 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.18.3-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_31 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.19.3-eksbuild.1".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_32 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.19.6-eksbuild.7".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_33 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsVpcCniAddon {
                    version: "v1.19.6-eksbuild.7".to_string(),
                },
            },
        ];

        for tc in tests_cases {
            // execute:
            let result = AwsVpcCniAddon::new_from_k8s_version(tc.k8s_version);

            // verify:
            assert_eq!(tc.expected, result);
        }
    }

    #[test]
    fn aws_addon_cni_new_with_overridden_version() {
        // setup:
        let tests_cases = vec!["toto", "v1.11.4-eksbuild.1", "v1.11.6-eksbuild.2"];

        for tc in tests_cases {
            // execute:
            let result = AwsVpcCniAddon::new_with_overridden_version(tc);

            // verify:
            assert_eq!(
                AwsVpcCniAddon {
                    version: tc.to_string()
                },
                result
            );
        }
    }
}
