use crate::cloud_provider::digitalocean::do_api_common::{do_get_from_api, DoApiType};
use crate::cloud_provider::digitalocean::models::doks::KubernetesCluster;
use crate::cloud_provider::digitalocean::models::doks::{DoksList, DoksOptions, KubernetesVersion};
use crate::cloud_provider::utilities::VersionsNumber;
use crate::error::{SimpleError, SimpleErrorKind, StringError};
use std::str::FromStr;

pub fn get_doks_info_from_name(
    json_content: &str,
    cluster_name: String,
) -> Result<Option<KubernetesCluster>, SimpleError> {
    let res_doks = serde_json::from_str::<DoksList>(json_content);

    match res_doks {
        Ok(clusters) => {
            let mut cluster_info = None;

            for cluster in clusters.kubernetes_clusters {
                if cluster.name == cluster_name {
                    cluster_info = Some(cluster);
                    break;
                }
            }

            if cluster_info.is_some() {
                info!("cluster {} is present from DigitalOcean API", cluster_name);
            } else {
                info!("cluster {} is not present from DigitalOcean API", cluster_name)
            }

            Ok(cluster_info)
        }
        Err(e) => Err(SimpleError {
            kind: SimpleErrorKind::Other,
            message: Some(format!(
                "error while trying to deserialize json received from Digital Ocean DOKS API. {}",
                e
            )),
        }),
    }
}

pub fn get_do_latest_doks_slug_from_api(token: &str, wished_version: &str) -> Result<Option<String>, SimpleError> {
    let api_url = format!("{}/options", DoApiType::Doks.api_url());

    let json_content = do_get_from_api(token, DoApiType::Doks, api_url)?;
    let doks_versions = get_doks_versions_from_api_output(&json_content)?;
    match get_do_kubernetes_latest_slug_version(&doks_versions, wished_version) {
        Ok(x) => Ok(x),
        Err(e) => Err(SimpleError {
            kind: SimpleErrorKind::Other,
            message: Some(format!(
                "version {} is not supported by DigitalOcean. {}",
                wished_version, e
            )),
        }),
    }
}

fn get_doks_versions_from_api_output(json_content: &str) -> Result<Vec<KubernetesVersion>, SimpleError> {
    let res_doks_options = serde_json::from_str::<DoksOptions>(json_content);

    match res_doks_options {
        Ok(options) => Ok(options.options.versions),
        Err(e) => Err(SimpleError {
            kind: SimpleErrorKind::Other,
            message: Some(format!(
                "error while trying to deserialize json received from Digital Ocean DOKS API. {}",
                e
            )),
        }),
    }
}

// get DOKS slug version from available DOKS versions
fn get_do_kubernetes_latest_slug_version(
    doks_versions: &[KubernetesVersion],
    wished_version: &str,
) -> Result<Option<String>, StringError> {
    let wished_k8s_version = VersionsNumber::from_str(wished_version)?;

    for kubernetes_doks_version in doks_versions {
        let current_k8s_version = VersionsNumber::from_str(kubernetes_doks_version.kubernetes_version.as_str())?;
        if current_k8s_version.major == wished_k8s_version.major
            && current_k8s_version.minor == wished_k8s_version.minor
        {
            return Ok(Some(kubernetes_doks_version.slug.clone()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests_doks {
    use crate::cloud_provider::digitalocean::kubernetes::doks_api::{
        get_do_kubernetes_latest_slug_version, get_doks_info_from_name, get_doks_versions_from_api_output,
    };

    fn do_get_doks_clusters() -> String {
        // https://docs.digitalocean.com/reference/api/api-reference/#tag/Kubernetes
        let content = r#"
{
  "kubernetes_clusters": [
    {
      "id": "08584879-cfe5-4b26-b29f-ee3765a6acfa",
      "name": "qovery-abcdefghi",
      "region": "nyc3",
      "version": "1.19.12-do.0",
      "cluster_subnet": "10.244.0.0/16",
      "service_subnet": "10.245.0.0/16",
      "vpc_uuid": "63ffbe86-315b-4d89-9fc4-e23d4378e865",
      "ipv4": "134.209.45.144",
      "endpoint": "https://08584297-cfe5-4b26-b29f-ee3765a6acfa.k8s.ondigitalocean.com",
      "tags": [
        "k8s",
        "k8s:08584879-cfe5-4b26-b29f-ee3765a6acfa"
      ],
      "node_pools": [
        {
          "id": "b8ca6136-db9b-405b-8f05-df432883631e",
          "name": "qovery-abcdefghi",
          "size": "s-4vcpu-8gb",
          "count": 4,
          "tags": [
            "k8s",
            "k8s:08584297-cfe5-4b26-b29f-ee3765a6acfa",
            "k8s:worker",
            "terraform:default-node-pool",
            "abcdefghi"
          ],
          "labels": null,
          "taints": [],
          "auto_scale": true,
          "min_nodes": 3,
          "max_nodes": 10,
          "nodes": [
            {
              "id": "f4ecafba-ee51-4604-befa-8fc27363b645",
              "name": "qovery-abcdefghi-8hh0w",
              "status": {
                "state": "running"
              },
              "droplet_id": "258847148",
              "created_at": "2021-08-09T14:45:08Z",
              "updated_at": "2021-08-09T14:48:01Z"
            },
            {
              "id": "f715584d-9c56-43e7-a6e8-509125322510",
              "name": "qovery-abcdefghi-8hh16",
              "status": {
                "state": "running"
              },
              "droplet_id": "258847741",
              "created_at": "2021-08-09T14:52:38Z",
              "updated_at": "2021-08-09T14:53:18Z"
            }
          ]
        }
      ],
      "maintenance_policy": {
        "start_time": "12:00",
        "duration": "4h0m0s",
        "day": "any"
      },
      "auto_upgrade": true,
      "status": {
        "state": "running"
      },
      "created_at": "2021-08-09T14:45:08Z",
      "updated_at": "2021-08-09T14:55:19Z",
      "surge_upgrade": true,
      "registry_enabled": true,
      "ha": false
    },
    {
      "id": "35c57d5d-6908-3a57-956e-799a7fec3e0f",
      "name": "k8s-1-20-2-do-0-ams3-1619016105608",
      "region": "ams3",
      "version": "1.20.2-do.0",
      "cluster_subnet": "10.244.0.0/16",
      "service_subnet": "10.245.0.0/16",
      "vpc_uuid": "aeb265f0-813d-4387-80c7-c96910b64597",
      "ipv4": "128.199.51.238",
      "endpoint": "https://35c57d5d-6908-4a27-956e-799a7fec3e0f.k8s.ondigitalocean.com",
      "tags": [
        "k8s",
        "k8s:35c57d5d-6908-3a57-956e-799a7fec3e0f"
      ],
      "node_pools": [
        {
          "id": "71e24830-2a70-4253-a3e1-0e81022b7fcc",
          "name": "pool-b7ok1pgfj",
          "size": "s-2vcpu-4gb",
          "count": 4,
          "tags": [
            "k8s",
            "k8s:35c57d5d-6908-4a27-956e-799a7fec3e0f",
            "k8s:worker"
          ],
          "labels": null,
          "taints": [],
          "auto_scale": false,
          "min_nodes": 0,
          "max_nodes": 0,
          "nodes": [
            {
              "id": "3ac9ffed-d404-4ed1-b2fa-6768b5a9a5ec",
              "name": "pool-b7ok1pgfj-8o3pe",
              "status": {
                "state": "running"
              },
              "droplet_id": "242572012",
              "created_at": "2021-04-21T14:42:27Z",
              "updated_at": "2021-04-21T14:46:54Z"
            },
            {
              "id": "85bd76d6-9ed2-44d3-9a8e-090e5a97a4e1",
              "name": "pool-b7ok1pgfj-8o3pa",
              "status": {
                "state": "running"
              },
              "droplet_id": "242572009",
              "created_at": "2021-04-21T14:42:27Z",
              "updated_at": "2021-04-21T14:46:54Z"
            }
          ]
        }
      ],
      "maintenance_policy": {
        "start_time": "3:00",
        "duration": "4h0m0s",
        "day": "any"
      },
      "auto_upgrade": false,
      "status": {
        "state": "running"
      },
      "created_at": "2021-04-21T14:42:27Z",
      "updated_at": "2021-08-09T03:00:51Z",
      "surge_upgrade": true,
      "registry_enabled": true,
      "ha": false
    },
    {
      "id": "c4b19427-a518-44a9-8bf0-b05f8d836eb3",
      "name": "qovery-adsayfusp6wdjjhw",
      "region": "nyc3",
      "version": "1.18.19-do.0",
      "cluster_subnet": "10.244.0.0/16",
      "service_subnet": "10.245.0.0/16",
      "vpc_uuid": "4d986a19-c26a-413b-ae4b-b8413126b24b",
      "ipv4": "138.197.110.182",
      "endpoint": "https://c4b19427-a518-44a9-8bf0-d05f8d836eb3.k8s.ondigitalocean.com",
      "tags": [
        "k8s",
        "k8s:c4b19427-a518-44a9-8bf0-b05f8d836eb3"
      ],
      "node_pools": [
        {
          "id": "d7147a6c-6cfd-4833-acef-56dcf4917ba3",
          "name": "qovery-adsayfusp6wdjjhw",
          "size": "s-4vcpu-8gb",
          "count": 1,
          "tags": [
            "adsayfusp6wdjjhw",
            "k8s",
            "k8s:c4b19427-a518-44a9-8bf0-d05f8d836eb3",
            "k8s:worker",
            "terraform:default-node-pool"
          ],
          "labels": null,
          "taints": [],
          "auto_scale": true,
          "min_nodes": 1,
          "max_nodes": 100,
          "nodes": [
            {
              "id": "b11968cb-e0f9-4b87-9c09-670b3a375f76",
              "name": "qovery-adsayfusp6wdjjhw-825nn",
              "status": {
                "state": "running"
              },
              "droplet_id": "253971918",
              "created_at": "2021-07-08T08:03:39Z",
              "updated_at": "2021-07-08T08:04:19Z"
            }
          ]
        }
      ],
      "maintenance_policy": {
        "start_time": "20:00",
        "duration": "4h0m0s",
        "day": "any"
      },
      "auto_upgrade": true,
      "status": {
        "state": "running"
      },
      "created_at": "2020-12-26T14:41:22Z",
      "updated_at": "2021-08-07T20:00:48Z",
      "surge_upgrade": true,
      "registry_enabled": true,
      "ha": false
    }
  ],
  "meta": {
    "total": 3
  },
  "links": {}
}
        "#;

        content.to_string()
    }

    fn do_get_doks_clusters_options() -> String {
        // https://docs.digitalocean.com/reference/api/api-reference/#tag/Kubernetes
        let content = r#"
{
  "options": {
    "regions": [
      {
        "name": "San Francisco 3",
        "slug": "sfo3"
      }
    ],
    "versions": [
      {
        "slug": "1.21.2-do.2",
        "kubernetes_version": "1.21.2"
      },
      {
        "slug": "1.20.8-do.0",
        "kubernetes_version": "1.20.8"
      },
      {
        "slug": "1.19.12-do.0",
        "kubernetes_version": "1.19.12"
      }
    ],
    "sizes": [
      {
        "name": "s-1vcpu-2gb",
        "slug": "s-1vcpu-2gb"
      }
    ]
  }
}
        "#;

        content.to_string()
    }

    #[test]
    fn do_get_do_cluster_id_from_name_api() {
        let json_content = do_get_doks_clusters();

        assert_eq!(
            get_doks_info_from_name(&json_content, "qovery-abcdefghi".to_string())
                .unwrap()
                .unwrap()
                .id,
            "08584879-cfe5-4b26-b29f-ee3765a6acfa".to_string()
        );
        assert!(get_doks_info_from_name(&json_content, "do-not-exists".to_string())
            .unwrap()
            .is_none());
    }

    #[test]
    fn check_doks_version_convert_to_slug() {
        let json_content = do_get_doks_clusters_options();
        let doks_versions = get_doks_versions_from_api_output(json_content.as_str()).unwrap();

        // not supported anymore version
        assert!(get_do_kubernetes_latest_slug_version(&doks_versions, "1.18")
            .unwrap()
            .is_none());
        // supported versions
        assert_eq!(
            get_do_kubernetes_latest_slug_version(&doks_versions, "1.19")
                .unwrap()
                .unwrap(),
            "1.19.12-do.0".to_string()
        );
        assert_eq!(
            get_do_kubernetes_latest_slug_version(&doks_versions, "1.21")
                .unwrap()
                .unwrap(),
            "1.21.2-do.2".to_string()
        );
    }
}
