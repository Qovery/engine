use crate::cloud_provider::kubernetes::Kind;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClusterSecretsIo {
    pub aws_access_key: String,
    pub aws_default_region: String,
    pub aws_secret_access_key: String,
    pub kubeconfig_b64: Option<String>,
    pub k8s_cluster_endpoint: Option<String>,
    pub cloud_provider: Kind,
    pub cluster_name: String,
    pub cluster_id: String,
    pub grafana_login: String,
    pub grafana_password: String,
    pub organization_id: String,
    pub test_cluster: String,
}

impl ClusterSecretsIo {
    pub fn new(
        aws_access_key: String,
        aws_default_region: String,
        aws_secret_access_key: String,
        kubeconfig_b64: Option<String>,
        k8s_cluster_endpoint: Option<String>,
        cloud_provider: Kind,
        cluster_name: String,
        cluster_id: String,
        grafana_login: String,
        grafana_password: String,
        organization_id: String,
        test_cluster: String,
    ) -> ClusterSecretsIo {
        ClusterSecretsIo {
            aws_access_key,
            aws_default_region,
            aws_secret_access_key,
            kubeconfig_b64,
            k8s_cluster_endpoint,
            cloud_provider,
            cluster_name,
            cluster_id,
            grafana_login,
            grafana_password,
            organization_id,
            test_cluster,
        }
    }
}
