#[cfg(any(feature = "test-aws-self-hosted", feature = "test-aws-managed-services"))]
mod aws_databases;
#[cfg(feature = "test-aws-minimal")]
mod aws_ecr;
#[cfg(any(feature = "test-aws-minimal", feature = "test-aws-self-hosted"))]
mod aws_environment;
#[cfg(feature = "test-aws-minimal")]
mod aws_environment_gateway_api;
#[cfg(any(
    feature = "test-aws-infra",
    feature = "test-aws-infra-arm",
    feature = "test-aws-infra-karpenter",
    feature = "test-aws-infra-nat-gateway",
    feature = "test-aws-infra-upgrade"
))]
mod aws_kubernetes;
#[cfg(any(feature = "test-aws-minimal", feature = "test-quarantine"))]
mod aws_s3;
#[cfg(any(feature = "test-aws-whole-enchilada", feature = "test-aws-whole-enchilada-gpu"))]
mod aws_whole_enchilada;
