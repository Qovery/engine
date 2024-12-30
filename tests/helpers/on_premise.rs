use qovery_engine::infrastructure::models::kubernetes::KubernetesVersion;

pub const ON_PREMISE_KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_30 {
    prefix: None,
    patch: None,
    suffix: None,
};