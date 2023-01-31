use crate::cloud_provider::kubernetes::{KubernetesAddon, KubernetesError};
use serde_derive::Serialize;

/// AWS EBS CSI addon
/// https://docs.aws.amazon.com/eks/latest/userguide/managing-ebs-csi.html
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsEbsCsiAddon {
    version: String,
}

impl AwsEbsCsiAddon {
    pub fn new_from_k8s_version(k8s_version: &str) -> Result<Self, KubernetesError> {
        Ok(AwsEbsCsiAddon {
            // Get current default build of an aws-ebs-csi add-on:
            // aws eks describe-addon-versions --kubernetes-version 1.22 --addon-name aws-ebs-csi-driver | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                "1.22" => "v1.14.0-eksbuild.1",
                "1.23" => "v1.15.0-eksbuild.1",
                _ => {
                    return Err(KubernetesError::AddonUnSupportedKubernetesVersion {
                        kubernetes_version: k8s_version.to_string(),
                        addon: KubernetesAddon::EbsCsi,
                    })
                }
            }
            .to_string(),
        })
    }

    pub fn new_with_overridden_version(addon_version: &str) -> Self {
        AwsEbsCsiAddon {
            version: addon_version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::addons::aws_ebs_csi_addon::AwsEbsCsiAddon;
    use crate::cloud_provider::kubernetes::{KubernetesAddon, KubernetesError};

    #[test]
    fn aws_addon_ebs_csi_new_test() {
        // setup:
        struct TestCase<'a> {
            k8s_version: &'a str,
            expected: Result<AwsEbsCsiAddon, KubernetesError>,
        }

        let tests_cases = vec![
            TestCase {
                k8s_version: "1.22",
                expected: Ok(AwsEbsCsiAddon {
                    version: "v1.14.0-eksbuild.1".to_string(),
                }),
            },
            TestCase {
                k8s_version: "1.23",
                expected: Ok(AwsEbsCsiAddon {
                    version: "v1.15.0-eksbuild.1".to_string(),
                }),
            },
            TestCase {
                k8s_version: "1.21",
                expected: Err(KubernetesError::AddonUnSupportedKubernetesVersion {
                    kubernetes_version: "1.21".to_string(),
                    addon: KubernetesAddon::EbsCsi,
                }),
            },
            TestCase {
                k8s_version: "1.24",
                expected: Err(KubernetesError::AddonUnSupportedKubernetesVersion {
                    kubernetes_version: "1.24".to_string(),
                    addon: KubernetesAddon::EbsCsi,
                }),
            },
        ];

        for tc in tests_cases {
            // execute:
            let result = AwsEbsCsiAddon::new_from_k8s_version(tc.k8s_version);

            // verify:
            assert_eq!(tc.expected, result);
        }
    }

    #[test]
    fn aws_addon_ebs_csi_new_with_overriden_version() {
        // setup:
        let tests_cases = vec!["toto", "v1.11.4-eksbuild.1", "v1.11.6-eksbuild.2"];

        for tc in tests_cases {
            // execute:
            let result = AwsEbsCsiAddon::new_with_overridden_version(tc);

            // verify:
            assert_eq!(
                AwsEbsCsiAddon {
                    version: tc.to_string()
                },
                result
            );
        }
    }
}
