use crate::error::SimpleError;

pub fn kubernetes_config_path(
    workspace_directory: &str,
    organization_id: &str,
    kubernetes_cluster_id: &str,
    token: &str,
) -> Result<String, SimpleError> {
    unimplemented!()
}
