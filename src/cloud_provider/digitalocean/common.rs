extern crate serde_json;

use std::fs;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;

use reqwest::StatusCode;

use crate::cloud_provider::digitalocean::models::cluster::Clusters;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::container_registry::docr::get_header_with_bearer;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::object_storage::do_space::download_space_object;
use crate::runtime;

pub const DO_CLUSTER_API_PATH: &str = "https://api.digitalocean.com/v2/kubernetes/clusters";

pub fn kubernetes_config_path(
    workspace_directory: &str,
    kubernetes_cluster_id: &str,
    region: &str,
    spaces_secret_key: &str,
    spaces_access_id: &str,
) -> Result<String, SimpleError> {
    let kubernetes_config_bucket_name = format!("qovery-kubeconfigs-{}", kubernetes_cluster_id);
    let kubernetes_config_object_key = format!("{}.yaml", kubernetes_cluster_id);

    let kubernetes_config_file_path = format!(
        "{}/kubernetes_config_{}",
        workspace_directory, kubernetes_cluster_id
    );

    // download kubeconfig file
    let _ = runtime::async_run(download_space_object(
        spaces_access_id,
        spaces_secret_key,
        kubernetes_config_bucket_name.as_str(),
        kubernetes_config_object_key.as_str(),
        region,
        kubernetes_config_file_path.as_str().clone(),
    ));

    // removes warning kubeconfig is (world/group) readable
    let file = File::open(kubernetes_config_file_path.clone().as_str())?;
    let metadata = file.metadata()?;

    let mut permissions = metadata.permissions();
    permissions.set_mode(0o400);

    fs::set_permissions(kubernetes_config_file_path.clone().as_str(), permissions)?;

    Ok(kubernetes_config_file_path.clone())
}

// retrieve the digital ocean uuid of the kube cluster from our cluster name
// each (terraform) apply may change the cluster uuid, so We need to retrieve it from the Digital Ocean API
pub fn get_uuid_of_cluster_from_name(
    token: &str,
    kube_cluster_name: &str,
) -> Result<String, SimpleError> {
    let headers = get_header_with_bearer(token);
    let res = reqwest::blocking::Client::new()
        .get(DO_CLUSTER_API_PATH)
        .headers(headers)
        .send();

    return match res {
        Ok(response) => match response.status() {
            StatusCode::OK => {
                let content = response.text().unwrap();
                let res_clusters = serde_json::from_str::<Clusters>(&content);
                match res_clusters {
                    Ok(clusters) => match search_uuid_cluster_for(kube_cluster_name, clusters) {
                        Some(uuid) => Ok(uuid),
                        None => Err(SimpleError::new(
                            SimpleErrorKind::Other,
                            Some(
                                "Unable to retrieve cluster id from this name",
                            ),
                        ))
                    }
                    Err(e) => {
                        print!("{}", e);
                        Err(SimpleError::new(
                            SimpleErrorKind::Other,
                            Some(
                                "While trying to deserialize json received from Digital Ocean API",
                            ),
                        ))
                    }
                }
            }
            _ => Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(
                    "Receive weird status Code from Digital Ocean while retrieving the cluster list",
                ),
            )),
        },
        Err(_) => {
            Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("Unable to get any responses from Digital Ocean"),
            ))
        }
    };
}

fn search_uuid_cluster_for(kube_name: &str, clusters: Clusters) -> Option<String> {
    for cluster in clusters.kubernetes_clusters {
        match cluster.name.eq(kube_name) {
            true => return Some(cluster.id),
            _ => {}
        }
    }
    None
}
