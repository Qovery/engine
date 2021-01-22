extern crate serde_json;

use reqwest::{StatusCode, Url};

use crate::cloud_provider::digitalocean::models::cluster::Clusters;
use crate::cloud_provider::digitalocean::models::load_balancers::LoadBalancer;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::utilities::get_header_with_bearer;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

pub const DO_CLUSTER_API_PATH: &str = "https://api.digitalocean.com/v2/kubernetes/clusters";
pub const DO_LOAD_BALANCER_API_PATH: &str = "https://api.digitalocean.com/v2/load_balancers";

pub fn do_get_load_balancer_ip(
    token: &str,
    load_balancer_id: &str,
) -> Result<Ipv4Addr, SimpleError> {
    let headers = get_header_with_bearer(token);
    let url = format!("{}/{}", DO_LOAD_BALANCER_API_PATH, load_balancer_id);
    let res = reqwest::blocking::Client::new()
        .get(&url)
        .headers(headers)
        .send();

    return match res {
        Ok(response) => match response.status() {
            StatusCode::OK => {
                let content = response.text().unwrap();
                let res_load_balancer = serde_json::from_str::<LoadBalancer>(&content);

                match res_load_balancer {
                    Ok(lb) => {
                        match Ipv4Addr::from_str(&lb.ip) {
                            Ok(ip) => Ok(ip),
                            Err(e) => {
                                error!("Info returned from DO API is not a valid IP, received '{}' instead: {}", lb.ip, e);
                                Err(SimpleError::new(
                                    SimpleErrorKind::Other,
                                    Some(
                                        format!("IP address of Digital Ocean Load Balancer given by the API is not valid: {}", e),
                                    ),
                                ))
                            }
                        }
                    },
                    Err(e) => {
                        print!("{}", e);
                        Err(SimpleError::new(
                            SimpleErrorKind::Other,
                            Some(
                                format!("Error While trying to deserialize json received from Digital Ocean Load Balancer API: {}", e),
                            ),
                        ))
                    }
                }
            }
            _ => Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(
                    "Unknown status code received from Digital Ocean Kubernetes API while retrieving load balancer information",
                ),
            )),
        },
        Err(_) => {
            Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("Unable to get a response from Digital Ocean Load Balancer API"),
            ))
        }
    };
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
