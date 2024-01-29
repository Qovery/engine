use crate::cloud_provider::kubernetes::KubernetesVersion;
use serde_derive::Serialize;

/// AWS EBS CSI addon
/// https://docs.aws.amazon.com/eks/latest/userguide/managing-ebs-csi.html
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsEbsCsiAddon {
    version: String,
}

impl AwsEbsCsiAddon {
    pub fn new_from_k8s_version(k8s_version: KubernetesVersion) -> Self {
        AwsEbsCsiAddon {
            // Get current default build of an aws-ebs-csi add-on:
            // aws eks describe-addon-versions --kubernetes-version 1.22 --addon-name aws-ebs-csi-driver | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                KubernetesVersion::V1_23 { .. } => "v1.15.0-eksbuild.1",
                KubernetesVersion::V1_24 { .. } => "v1.19.0-eksbuild.1",
                KubernetesVersion::V1_25 { .. } => "v1.19.0-eksbuild.2",
                KubernetesVersion::V1_26 { .. } => "v1.20.0-eksbuild.1",
                KubernetesVersion::V1_27 { .. } => "v1.26.1-eksbuild.1",
                KubernetesVersion::V1_28 { .. } => "v1.27.0-eksbuild.1",
            }
            .to_string(),
        }
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
    use crate::cloud_provider::kubernetes::KubernetesVersion;

    #[test]
    fn aws_addon_ebs_csi_new_test() {
        // setup:
        struct TestCase {
            k8s_version: KubernetesVersion,
            expected: AwsEbsCsiAddon,
        }

        let tests_cases = vec![TestCase {
            k8s_version: KubernetesVersion::V1_26 {
                prefix: None,
                patch: None,
                suffix: None,
            },
            expected: AwsEbsCsiAddon {
                version: "v1.20.0-eksbuild.1".to_string(),
            },
        }];

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
