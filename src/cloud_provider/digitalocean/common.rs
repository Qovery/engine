use crate::cloud_provider::digitalocean::api_structs::clusters::Clusters;
use crate::cloud_provider::digitalocean::DO;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::constants::DIGITAL_OCEAN_TOKEN;
use crate::container_registry::docr::get_header_with_bearer;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::object_storage::do_space::download_space_object;
use reqwest::StatusCode;
use std::os::unix::fs::PermissionsExt;
extern crate serde_json;
use std::fs;
use std::fs::File;
use tokio::runtime::Runtime;

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

    let future_kubeconfig = download_space_object(
        spaces_access_id,
        spaces_secret_key,
        kubernetes_config_bucket_name.as_str(),
        kubernetes_config_object_key.as_str(),
        region,
        kubernetes_config_file_path.as_str().clone(),
    );
    Runtime::new()
        .expect("Failed to create Tokio runtime to download kubeconfig")
        .block_on(future_kubeconfig);
    // removes warning kubeconfig is (world/group) readable

    let mut file = File::open(kubernetes_config_file_path.clone().as_str())?;
    let metadata = file.metadata()?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o400);
    fs::set_permissions(kubernetes_config_file_path.clone().as_str(), permissions)?;

    Ok(kubernetes_config_file_path.clone())
}

pub const do_cluster_api_path: &str = "https://api.digitalocean.com/v2/kubernetes/clusters";

/*
Waiting for https://github.com/pandaman64/serde-query/issues/2
#[derive(serde_query::Deserialize)]
struct Cluster {
    #[query(r#".["kubernetes_clusters"].id"#)]
    cluster_id: String,
    #[query(r#".["kubernetes_clusters"].name"#)]
    cluster_name: String,
}
*/

// retrieve the digital ocean uuid of the kube cluster from our cluster name
// each (terraform) apply may change the cluster uuid, so We need to retrieve it from the Digital Ocean API
pub fn get_uuid_of_cluster_from_name(
    token: &str,
    kube_cluster_name: &str,
) -> Result<String, SimpleError> {
    let mut headers = get_header_with_bearer(token);
    let res = reqwest::blocking::Client::new()
        .get(do_cluster_api_path)
        .headers(headers)
        .send();
    match res {
        Ok(response) => match response.status() {
            StatusCode::OK => {
                let content = response.text().unwrap();
                let res_clusters  = serde_json::from_str::<Clusters>(&content);
                match res_clusters{
                    Ok(clusters) => match search_uuid_cluster_for(kube_cluster_name,clusters){
                        Some(uuid) => return Ok(uuid),
                        None => return Err(SimpleError::new(
                            SimpleErrorKind::Other,
                            Some(
                                "Unable to retrieve cluster id from this name",
                            ),
                        ))
                    }
                    Err(e) => {
                        print!("{}", e);
                        return Err(SimpleError::new(
                            SimpleErrorKind::Other,
                            Some(
                                "While trying to deserialize json received from Digital Ocean API",
                            ),
                        ));
                    },
                }
            }
            _ => return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(
                    "Receive weird status Code from Digital Ocean while retrieving the cluster list",
                ),
            )),
        },
        Err(_) => {
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("Unable to get any responses from Digital Ocean"),
            ))
        }
    }
}

fn search_uuid_cluster_for(kubeName: &str, clusters: Clusters) -> Option<String> {
    for cluster in clusters.kubernetes_clusters {
        match cluster.name.eq(kubeName) {
            true => return Some(cluster.id),
            _ => {}
        }
    }
    None
}

pub fn do_stateless_service_cleanup(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    workspace_dir: &str,
    helm_release_name: &str,
) -> Result<(), SimpleError> {
    let digitalocean = kubernetes
        .cloud_provider()
        .as_any()
        .downcast_ref::<DO>()
        .unwrap();

    let kubernetes_config_file_path = kubernetes_config_path(
        workspace_dir,
        environment.organization_id.as_str(),
        kubernetes.id(),
        digitalocean.spaces_secret_key.as_str(),
        digitalocean.spaces_access_id.as_str(),
    )?;

    let do_credentials_envs = vec![(DIGITAL_OCEAN_TOKEN, digitalocean.token.as_str())];

    let history_rows = crate::cmd::helm::helm_exec_history(
        kubernetes_config_file_path.as_str(),
        environment.namespace(),
        helm_release_name,
        do_credentials_envs.clone(),
    )?;

    // if there is no valid history - then delete the helm chart
    let first_valid_history_row = history_rows.iter().find(|x| x.is_successfully_deployed());

    if first_valid_history_row.is_some() {
        crate::cmd::helm::helm_exec_uninstall(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name,
            do_credentials_envs,
        )?;
    }

    Ok(())
}
