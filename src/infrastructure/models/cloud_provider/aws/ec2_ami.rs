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
}
