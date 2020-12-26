extern crate serde_json;

use reqwest::StatusCode;

use crate::cloud_provider::digitalocean::models::cluster::Clusters;
use crate::container_registry::docr::get_header_with_bearer;
use crate::error::{SimpleError, SimpleErrorKind};

pub const DO_CLUSTER_API_PATH: &str = "https://api.digitalocean.com/v2/kubernetes/clusters";

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
                                format!("Unable to retrieve cluster id from the cluster name {}", kube_cluster_name),
                            ),
                        ))
                    }
                    Err(e) => {
                        print!("{}", e);
                        Err(SimpleError::new(
                            SimpleErrorKind::Other,
                            Some(
                                "While trying to deserialize json received from Digital Ocean Kubernetes API",
                            ),
                        ))
                    }
                }
            }
            _ => Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(
                    "Receive unknown status code from Digital Ocean Kubernetes API while retrieving clusters list",
                ),
            )),
        },
        Err(_) => {
            Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("Unable to get a response from Digital Ocean Kubernetes API"),
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
