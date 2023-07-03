use crate::cloud_provider::kubernetes::KubernetesVersion;
use serde_derive::Serialize;

/// AWS Kube proxy addon
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsKubeProxyAddon {
    version: String,
}

impl AwsKubeProxyAddon {
    pub fn new_from_k8s_version(k8s_version: KubernetesVersion) -> Self {
        AwsKubeProxyAddon {
            // Get current default build of an kube-proxy add-on:
            // https://docs.aws.amazon.com/en_us/eks/latest/userguide/managing-kube-proxy.html
            // aws eks describe-addon-versions --kubernetes-version 1.25 --addon-name kube-proxy | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                KubernetesVersion::V1_22 { .. } => "v1.22.17-eksbuild.2",
                KubernetesVersion::V1_23 { .. } => "v1.23.16-eksbuild.2",
                KubernetesVersion::V1_24 { .. } => "v1.24.10-eksbuild.2",
                KubernetesVersion::V1_25 { .. } => "v1.25.6-eksbuild.1",
            }
            .to_string(),
        }
    }

    pub fn new_with_overridden_version(addon_version: &str) -> Self {
        AwsKubeProxyAddon {
            version: addon_version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::addons::aws_kube_proxy::AwsKubeProxyAddon;
    use crate::cloud_provider::kubernetes::KubernetesVersion;

    #[test]
    fn aws_addon_kube_proxy_new_test() {
        // setup:
        struct TestCase {
            k8s_version: KubernetesVersion,
            expected: AwsKubeProxyAddon,
        }

        let tests_cases = vec![
            TestCase {
                k8s_version: KubernetesVersion::V1_24 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsKubeProxyAddon {
                    version: "v1.24.10-eksbuild.2".to_string(),
                },
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_25 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                expected: AwsKubeProxyAddon {
                    version: "v1.25.6-eksbuild.1".to_string(),
                },
            },
        ];

        for tc in tests_cases {
            // execute:
            let result = AwsKubeProxyAddon::new_from_k8s_version(tc.k8s_version);

            // verify:
            assert_eq!(tc.expected, result);
        }
    }

    #[test]
    fn aws_addon_kube_proxy_new_with_overridden_version() {
        // setup:
        let tests_cases = vec!["toto", "v1.24.10-eksbuild.1", "v1.25.6-eksbuild.2"];

        for tc in tests_cases {
            // execute:
            let result = AwsKubeProxyAddon::new_with_overridden_version(tc);

            // verify:
            assert_eq!(
                AwsKubeProxyAddon {
                    version: tc.to_string()
                },
                result
            );
        }
    }
}
