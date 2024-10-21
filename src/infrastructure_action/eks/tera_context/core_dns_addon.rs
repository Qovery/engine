use crate::cloud_provider::kubernetes::KubernetesVersion;
use serde_derive::Serialize;

/// AWS COREDNS addon
/// https://docs.aws.amazon.com/eks/latest/userguide/managing-coredns.html
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsCoreDnsAddon {
    version: String,
}

impl AwsCoreDnsAddon {
    pub fn new_from_k8s_version(k8s_version: KubernetesVersion) -> Self {
        AwsCoreDnsAddon {
            // Get current default build of an aws-codedns add-on:
            // aws eks describe-addon-versions --kubernetes-version 1.22 --addon-name aws-coredns | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                KubernetesVersion::V1_23 { .. } => "v1.8.7-eksbuild.10",
                KubernetesVersion::V1_24 { .. } => "v1.9.3-eksbuild.11",
                KubernetesVersion::V1_25 { .. } => "v1.9.3-eksbuild.11",
                KubernetesVersion::V1_26 { .. } => "v1.9.3-eksbuild.11",
                KubernetesVersion::V1_27 { .. } => "v1.10.1-eksbuild.7",
                KubernetesVersion::V1_28 { .. } => "v1.10.1-eksbuild.7",
                KubernetesVersion::V1_29 { .. } => "v1.10.1-eksbuild.7",
            }
            .to_string(),
        }
    }

    pub fn new_with_overridden_version(addon_version: &str) -> Self {
        AwsCoreDnsAddon {
            version: addon_version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_provider::kubernetes::KubernetesVersion;

    #[test]
    fn aws_addon_coredns_new_test() {
        // setup:
        struct TestCase {
            k8s_version: KubernetesVersion,
            expected: AwsCoreDnsAddon,
        }

        let tests_cases = vec![
            TestCase {
                k8s_version: KubernetesVersion::V1_23 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsCoreDnsAddon {
                    version: "v1.8.7-eksbuild.10".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_24 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsCoreDnsAddon {
                    version: "v1.9.3-eksbuild.11".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_25 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsCoreDnsAddon {
                    version: "v1.9.3-eksbuild.11".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_26 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsCoreDnsAddon {
                    version: "v1.9.3-eksbuild.11".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_27 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsCoreDnsAddon {
                    version: "v1.10.1-eksbuild.7".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_28 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsCoreDnsAddon {
                    version: "v1.10.1-eksbuild.7".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_29 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsCoreDnsAddon {
                    version: "v1.10.1-eksbuild.7".to_string(),
                },
            },
        ];

        for tc in tests_cases {
            // execute:
            let result = AwsCoreDnsAddon::new_from_k8s_version(tc.k8s_version);

            // verify:
            assert_eq!(tc.expected, result);
        }
    }

    #[test]
    fn aws_addon_coredns_new_with_overriden_version() {
        // setup:
        let tests_cases = vec!["toto", "v1.8.7-eksbuild.10", "v1.8.7-eksbuild.11"];

        for tc in tests_cases {
            // execute:
            let result = AwsCoreDnsAddon::new_with_overridden_version(tc);

            // verify:
            assert_eq!(
                AwsCoreDnsAddon {
                    version: tc.to_string()
                },
                result
            );
        }
    }
}
