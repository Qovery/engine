use crate::infrastructure::models::kubernetes::KubernetesVersion;
use crate::io_models::models::CpuArchitecture;
use std::fmt::{Display, Formatter};

#[derive(PartialEq, Eq)]
pub enum Ec2Ami {
    AmazonLinux2,
    AmazonLinux2023,
    Bottlerocket,
}

impl Ec2Ami {
    pub fn ami_type(&self, arch: CpuArchitecture) -> &str {
        match (self, arch) {
            (Ec2Ami::AmazonLinux2, CpuArchitecture::AMD64) => "AL2_x86_64",
            (Ec2Ami::AmazonLinux2, CpuArchitecture::ARM64) => "AL2_ARM_64",
            (Ec2Ami::AmazonLinux2023, CpuArchitecture::AMD64) => "AL2023_x86_64_STANDARD",
            (Ec2Ami::AmazonLinux2023, CpuArchitecture::ARM64) => "AL2023_ARM_64_STANDARD",
            (Ec2Ami::Bottlerocket, CpuArchitecture::ARM64) => "BOTTLEROCKET_ARM_64",
            (Ec2Ami::Bottlerocket, CpuArchitecture::AMD64) => "BOTTLEROCKET_x86_64",
        }
    }

    pub fn ami_selector_terms_alias(&self) -> &str {
        match self {
            Ec2Ami::AmazonLinux2 => "al2@latest",
            Ec2Ami::AmazonLinux2023 => "al2023@latest",
            Ec2Ami::Bottlerocket => "bottlerocket@latest",
        }
    }

    pub fn ami_selector_terms_name(
        &self,
        kubernetes_versions: &KubernetesVersion,
        arch: Option<CpuArchitecture>,
        gpu: bool,
    ) -> String {
        let flavor = match gpu {
            true => "nvidia",
            false => "standard",
        };

        match (self, arch) {
            (Ec2Ami::AmazonLinux2, None) => format!("amazon-eks-node-*-{flavor}-{kubernetes_versions}*"),
            (Ec2Ami::AmazonLinux2, Some(CpuArchitecture::AMD64)) => {
                format!("amazon-eks-node-x86_64-{flavor}-{kubernetes_versions}*")
            }
            (Ec2Ami::AmazonLinux2, Some(CpuArchitecture::ARM64)) => {
                format!("amazon-eks-node-arm64-{flavor}-{kubernetes_versions}*")
            }
            (Ec2Ami::AmazonLinux2023, None) => format!("amazon-eks-node-al2023-*-{flavor}-{kubernetes_versions}*"),
            (Ec2Ami::AmazonLinux2023, Some(CpuArchitecture::AMD64)) => {
                format!("amazon-eks-node-al2023-x86_64-{flavor}-{kubernetes_versions}*")
            }
            (Ec2Ami::AmazonLinux2023, Some(CpuArchitecture::ARM64)) => {
                format!("amazon-eks-node-al2023-arm64-{flavor}-{kubernetes_versions}*")
            }
            (Ec2Ami::Bottlerocket, None) => format!("bottlerocket-aws-k8s-{kubernetes_versions}-{flavor}-*"),
            (Ec2Ami::Bottlerocket, Some(CpuArchitecture::AMD64)) => {
                format!("bottlerocket-aws-k8s-{kubernetes_versions}-{flavor}-x86_64-*")
            }
            (Ec2Ami::Bottlerocket, Some(CpuArchitecture::ARM64)) => {
                format!("bottlerocket-aws-k8s-{kubernetes_versions}-{flavor}-aarch64-*")
            }
        }
    }
}

impl Display for Ec2Ami {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let ami_str = match self {
            Ec2Ami::AmazonLinux2 => "AL2",
            Ec2Ami::AmazonLinux2023 => "AL2023",
            Ec2Ami::Bottlerocket => "Bottlerocket",
        };
        write!(f, "{ami_str}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ami_type() {
        let ami = Ec2Ami::AmazonLinux2;
        assert_eq!(ami.ami_type(CpuArchitecture::AMD64), "AL2_x86_64");
        assert_eq!(ami.ami_type(CpuArchitecture::ARM64), "AL2_ARM_64");

        let ami = Ec2Ami::AmazonLinux2023;
        assert_eq!(ami.ami_type(CpuArchitecture::AMD64), "AL2023_x86_64_STANDARD");
        assert_eq!(ami.ami_type(CpuArchitecture::ARM64), "AL2023_ARM_64_STANDARD");

        let ami = Ec2Ami::Bottlerocket;
        assert_eq!(ami.ami_type(CpuArchitecture::AMD64), "BOTTLEROCKET_x86_64");
        assert_eq!(ami.ami_type(CpuArchitecture::ARM64), "BOTTLEROCKET_ARM_64");
    }

    #[test]
    fn test_ami_selector_terms_alias() {
        let ami = Ec2Ami::AmazonLinux2;
        assert_eq!(ami.ami_selector_terms_alias(), "al2@latest");

        let ami = Ec2Ami::AmazonLinux2023;
        assert_eq!(ami.ami_selector_terms_alias(), "al2023@latest");

        let ami = Ec2Ami::Bottlerocket;
        assert_eq!(ami.ami_selector_terms_alias(), "bottlerocket@latest");
    }

    #[test]
    fn test_to_string() {
        let ami = Ec2Ami::AmazonLinux2;
        assert_eq!(ami.to_string(), "AL2");

        let ami = Ec2Ami::AmazonLinux2023;
        assert_eq!(ami.to_string(), "AL2023");

        let ami = Ec2Ami::Bottlerocket;
        assert_eq!(ami.to_string(), "Bottlerocket");
    }

    const K8S_TEST_VERSION: KubernetesVersion = KubernetesVersion::V1_33 {
        prefix: None,
        patch: None,
        suffix: None,
    };

    #[test]
    fn test_ami_selector_terms_name_amazon_linux2_none_arch_standard() {
        let ami = Ec2Ami::AmazonLinux2;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, None, false);
        assert_eq!(result, "amazon-eks-node-*-standard-1.33*");
    }

    #[test]
    fn test_ami_selector_terms_name_amazon_linux2_x86_64_nvidia() {
        let ami = Ec2Ami::AmazonLinux2;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, Some(CpuArchitecture::AMD64), true);
        assert_eq!(result, "amazon-eks-node-x86_64-nvidia-1.33*");
    }

    #[test]
    fn test_ami_selector_terms_name_amazon_linux2023_arm64_standard() {
        let ami = Ec2Ami::AmazonLinux2023;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, Some(CpuArchitecture::ARM64), false);
        assert_eq!(result, "amazon-eks-node-al2023-arm64-standard-1.33*");
    }

    #[test]
    fn test_ami_selector_terms_name_bottlerocket_none_arch_nvidia() {
        let ami = Ec2Ami::Bottlerocket;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, None, true);
        assert_eq!(result, "bottlerocket-aws-k8s-1.33-nvidia-*");
    }

    #[test]
    fn test_ami_selector_terms_name_bottlerocket_x86_64_standard() {
        let ami = Ec2Ami::Bottlerocket;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, Some(CpuArchitecture::AMD64), false);
        assert_eq!(result, "bottlerocket-aws-k8s-1.33-standard-x86_64-*");
    }

    #[test]
    fn test_ami_selector_terms_name_bottlerocket_arm64_nvidia() {
        let ami = Ec2Ami::Bottlerocket;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, Some(CpuArchitecture::ARM64), true);
        assert_eq!(result, "bottlerocket-aws-k8s-1.33-nvidia-aarch64-*");
    }
}
