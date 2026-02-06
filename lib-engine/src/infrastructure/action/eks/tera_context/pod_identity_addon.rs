use crate::infrastructure::models::kubernetes::KubernetesVersion;
use serde_derive::Serialize;

/// AWS EKS Pod Identity Agent addon
/// https://docs.aws.amazon.com/eks/latest/userguide/pod-id-agent-setup.html
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsPodIdentityAddon {
    version: String,
}

impl AwsPodIdentityAddon {
    pub fn new_from_k8s_version(k8s_version: KubernetesVersion) -> Self {
        AwsPodIdentityAddon {
            // Get current default build of an eks-pod-identity-agent add-on:
            // aws eks describe-addon-versions --kubernetes-version 1.28 --addon-name eks-pod-identity-agent | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                KubernetesVersion::V1_23 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_24 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_25 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_26 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_27 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_28 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_29 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_30 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_31 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_32 { .. } => "v1.3.10-eksbuild.2",
                KubernetesVersion::V1_33 { .. } => "v1.3.10-eksbuild.2",
            }
            .to_string(),
        }
    }

    pub fn new_with_overridden_version(addon_version: &str) -> Self {
        AwsPodIdentityAddon {
            version: addon_version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::models::kubernetes::KubernetesVersion;

    #[test]
    fn aws_addon_pod_identity_new_test() {
        // setup:
        struct TestCase {
            k8s_version: KubernetesVersion,
            expected: AwsPodIdentityAddon,
        }

        let tests_cases = vec![
            TestCase {
                k8s_version: KubernetesVersion::V1_23 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_24 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_25 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_26 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_27 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_28 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_29 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_30 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_31 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_32 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_33 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsPodIdentityAddon {
                    version: "v1.3.10-eksbuild.2".to_string(),
                },
            },
        ];

        for tc in tests_cases {
            // execute:
            let result = AwsPodIdentityAddon::new_from_k8s_version(tc.k8s_version);

            // verify:
            assert_eq!(tc.expected, result);
        }
    }

    #[test]
    fn aws_addon_pod_identity_new_with_overridden_version() {
        // setup:
        let tests_cases = vec!["toto", "v1.3.10-eksbuild.2", "v1.3.2-eksbuild.2"];

        for tc in tests_cases {
            // execute:
            let result = AwsPodIdentityAddon::new_with_overridden_version(tc);

            // verify:
            assert_eq!(
                AwsPodIdentityAddon {
                    version: tc.to_string()
                },
                result
            );
        }
    }
}
