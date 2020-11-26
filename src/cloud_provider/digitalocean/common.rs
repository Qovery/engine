use crate::error::SimpleError;
use crate::object_storage::do_space::download_space_object;
use std::fs::File;
use std::io::Write;

pub fn kubernetes_config_path(
    workspace_directory: &str,
    organization_id: &str,
    kubernetes_cluster_id: &str,
    spaces_secret_key: &str,
    spaces_access_id: &str,
) -> Result<String, SimpleError> {
    let kubernetes_config_bucket_name = format!("qovery-kubeconfigs-{}", kubernetes_cluster_id);
    let kubernetes_config_object_key = format!("{}.yaml", kubernetes_cluster_id);

    let kubernetes_config_file_path = format!(
        "{}/kubernetes_config_{}",
        workspace_directory, kubernetes_cluster_id
    );

    let kubeconfig = download_space_object(
        spaces_access_id,
        spaces_secret_key,
        kubernetes_config_bucket_name.as_str(),
        kubernetes_config_object_key.as_str(),
    );
    match kubeconfig {
        Ok(body) => {
            let mut file =
                File::create(kubernetes_config_file_path.clone()).expect("unable to create file");
            file.write_all(body.as_bytes()).expect("unable to write");
            Ok(kubernetes_config_file_path)
        }
        Err(e) => Err(e),
    }
}
