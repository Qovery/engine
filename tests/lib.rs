#![allow(dead_code)]
#![allow(unused_imports)]

#[macro_use]
extern crate maplit;
extern crate core;

#[cfg(any(
    feature = "test-aws-minimal",
    feature = "test-aws-self-hosted",
    feature = "test-aws-managed-services",
    feature = "test-aws-whole-enchilada",
    feature = "test-aws-whole-enchilada-gpu",
    feature = "test-aws-infra",
    feature = "test-aws-infra-arm",
    feature = "test-aws-infra-karpenter",
    feature = "test-aws-infra-nat-gateway",
    feature = "test-aws-infra-upgrade",
    feature = "test-quarantine"
))]
mod aws;
#[cfg(any(
    feature = "test-azure-minimal",
    feature = "test-azure-self-hosted",
    feature = "test-azure-managed-services",
    feature = "test-azure-whole-enchilada",
    feature = "test-azure-infra",
    feature = "test-azure-infra-upgrade"
))]
mod azure;
#[cfg(feature = "test-local-docker")]
mod container_registries;
#[cfg(any(
    feature = "test-gcp-minimal",
    feature = "test-gcp-self-hosted",
    feature = "test-gcp-managed-services",
    feature = "test-gcp-whole-enchilada",
    feature = "test-gcp-infra",
    feature = "test-gcp-infra-upgrade",
    feature = "test-quarantine"
))]
mod gcp;
#[cfg(feature = "test-local-kube")]
mod helm;
pub mod helpers;
#[cfg(any(feature = "test-aws-minimal", feature = "test-aws-self-hosted"))]
mod kube;
#[cfg(feature = "test-aws-minimal")]
mod promtail;
#[cfg(any(
    feature = "test-scw-minimal",
    feature = "test-scw-self-hosted",
    feature = "test-scw-managed-services",
    feature = "test-scw-whole-enchilada",
    feature = "test-scw-infra",
    feature = "test-scw-infra-nat-gateway",
    feature = "test-scw-infra-upgrade",
    feature = "test-quarantine"
))]
mod scaleway;
#[cfg(any(
    feature = "test-aws-minimal",
    feature = "test-aws-self-hosted",
    feature = "test-azure-minimal",
    feature = "test-azure-self-hosted",
    feature = "test-gcp-minimal",
    feature = "test-scw-minimal"
))]
mod services;
