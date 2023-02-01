use crate::cloud_provider::kubernetes::{KubernetesAddon, KubernetesError};
use serde_derive::Serialize;

/// AWS VPC CNI addon
/// https://docs.aws.amazon.com/fr_fr/eks/latest/userguide/managing-vpc-cni.html
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsVpcCniAddon {
    version: String,
}

impl AwsVpcCniAddon {
    pub fn new_from_k8s_version(k8s_version: &str) -> Result<Self, KubernetesError> {
        Ok(AwsVpcCniAddon {
            // Get current default build of an aws-cni add-on:
            // aws eks describe-addon-versions --kubernetes-version 1.23 --addon-name vpc-cni | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                "1.22" => "v1.11.4-eksbuild.1",
                "1.23" => "v1.12.1-eksbuild.1",
                _ => {
                    return Err(KubernetesError::AddonUnSupportedKubernetesVersion {
                        kubernetes_version: k8s_version.to_string(),
                        addon: KubernetesAddon::Cni,
                    })
                }
            }
            .to_string(),
        })
    }

    pub fn new_with_overridden_version(addon_version: &str) -> Self {
        AwsVpcCniAddon {
            version: addon_version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::addons::aws_vpc_cni_addon::AwsVpcCniAddon;
    use crate::cloud_provider::kubernetes::{KubernetesAddon, KubernetesError};

    #[test]
    fn aws_addon_cni_new_test() {
        // setup:
        struct TestCase<'a> {
            k8s_version: &'a str,
            expected: Result<AwsVpcCniAddon, KubernetesError>,
        }

        let tests_cases = vec![
            TestCase {
                k8s_version: "1.22",
                expected: Ok(AwsVpcCniAddon {
                    version: "v1.11.4-eksbuild.1".to_string(),
                }),
            },
            TestCase {
                k8s_version: "1.23",
                expected: Ok(AwsVpcCniAddon {
                    version: "v1.12.1-eksbuild.1".to_string(),
                }),
            },
            TestCase {
                k8s_version: "1.21",
                expected: Err(KubernetesError::AddonUnSupportedKubernetesVersion {
                    kubernetes_version: "1.21".to_string(),
                    addon: KubernetesAddon::Cni,
                }),
            },
            TestCase {
                k8s_version: "1.24",
                expected: Err(KubernetesError::AddonUnSupportedKubernetesVersion {
                    kubernetes_version: "1.24".to_string(),
                    addon: KubernetesAddon::Cni,
                }),
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
    fn aws_addon_cni_new_with_overriden_version() {
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
