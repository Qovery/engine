use rusoto_core::Region;
use std::io::Error;
use std::str::FromStr;

pub fn kubernetes_config_path(
    workspace_directory: &str,
    organization_id: &str,
    kubernetes_cluster_id: &str,
    access_key_id: &str,
    secret_access_key: &str,
    region: &str,
) -> Result<String, Error> {
    let kubernetes_config_bucket_name = format!("kubeconfigs-{}", organization_id);
    let kubernetes_config_object_key = format!("{}.yaml", kubernetes_cluster_id);

    let kubernetes_config_file_path = format!(
        "{}/kubernetes_config_{}",
        workspace_directory, kubernetes_cluster_id
    );

    let _region = Region::from_str(region).unwrap();

    let _ = crate::s3::get_kubernetes_config_file(
        access_key_id,
        secret_access_key,
        &_region,
        kubernetes_config_bucket_name.as_str(),
        kubernetes_config_object_key.as_str(),
        kubernetes_config_file_path.as_str(),
    )?;

    Ok(kubernetes_config_file_path)
}
