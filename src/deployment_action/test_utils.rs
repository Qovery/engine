use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::autoscaling::v1::HorizontalPodAutoscaler;
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, PostParams};
use kube::Api;
use std::sync::{Arc, Barrier};
use std::thread;

pub fn get_simple_deployment() -> Deployment {
    serde_json::from_value(serde_json::json!({
      "apiVersion": "apps/v1",
      "kind": "Deployment",
      "metadata": {
        "name": "pause",
        "labels": {
          "app": "pause"
        }
      },
      "spec": {
        "replicas": 1,
        "selector": {
          "matchLabels": {
            "app": "pause"
          }
        },
        "template": {
          "metadata": {
            "labels": {
              "app": "pause"
            }
          },
          "spec": {
            "containers": [
              {
                "name": "pause",
                "image": "registry.k8s.io/pause:3.10"
              }
            ]
          }
        }
      }
    }))
    .unwrap()
}

pub fn get_simple_statefulset() -> StatefulSet {
    serde_json::from_value(serde_json::json!({
      "apiVersion": "apps/v1",
      "kind": "StatefulSet",
      "metadata": {
        "name": "pause",
        "labels": {
          "app": "pause"
        }
      },
      "spec": {
        "replicas": 1,
        "selector": {
          "matchLabels": {
            "app": "pause"
          }
        },
        "template": {
          "metadata": {
            "labels": {
              "app": "pause"
            }
          },
          "spec": {
            "containers": [
              {
                "name": "pause",
                "image": "registry.k8s.io/pause:3.9"
              }
            ]
          }
        }
      }
    }))
    .unwrap()
}

pub fn get_simple_cron_job() -> CronJob {
    serde_json::from_value(serde_json::json!({
           "apiVersion":"batch/v1",
           "kind":"CronJob",
           "metadata":{
              "name":"pause",
              "labels":{
                 "app":"pause"
              }
           },
           "spec":{
              "schedule":"*/5 * * * *",
              "jobTemplate":{
                 "spec":{
                    "template":{
                       "spec":{
                          "containers":[
                             {
                                "name":"pause",
                                "image":"registry.k8s.io/pause:3.9"
                             }
                          ],
                          "restartPolicy":"OnFailure"
                       }
                    }
                 }
              }
            }
    }))
    .unwrap()
}

pub fn get_simple_daemon_set() -> DaemonSet {
    serde_json::from_value(serde_json::json!({
           "apiVersion":"apps/v1",
           "kind":"DaemonSet",
           "metadata":{
              "name":"restart",
              "labels":{
                 "app":"restart"
              }
           },
            "spec": {
                "selector": {
                  "matchLabels": {
                    "app": "restart"
                  }
                },
                "template": {
                  "metadata": {
                    "labels": {
                      "app": "restart"
                    }
                  },
                  "spec": {
                    "containers": [
                      {
                        "name": "pause",
                        "image": "registry.k8s.io/pause:3.9"
                      }
                    ]
                  }
                }
      }
    }))
    .unwrap()
}

pub fn get_simple_hpa() -> HorizontalPodAutoscaler {
    serde_json::from_value(serde_json::json!({
    "apiVersion": "autoscaling/v1",
    "kind": "HorizontalPodAutoscaler",
    "metadata": {
      "name": "pause-hpa",
      "labels": {
        "app": "pause"
      }
    },
    "spec": {
      "scaleTargetRef": {
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "name": "pause"
      },
      "minReplicas": 1,
      "maxReplicas": 5,
      "targetCPUUtilizationPercentage": 80
    }
      }))
    .unwrap()
}

#[derive(Clone, Debug)]
pub struct NamespaceForTest {
    ns: Api<Namespace>,
    name: String,
}

impl NamespaceForTest {
    pub async fn new(kube_client: kube::Client, name: String) -> Result<NamespaceForTest, kube::Error> {
        let sel = NamespaceForTest {
            ns: Api::all(kube_client),
            name,
        };
        sel.ns
            .create(&PostParams::default(), &get_namespace(sel.name.clone()))
            .await?;
        Ok(sel)
    }
}

impl Drop for NamespaceForTest {
    fn drop(&mut self) {
        let ns = self.ns.clone();
        let name = self.name.clone();
        let stopped = Arc::new(Barrier::new(2));
        let s = stopped.clone();

        let handle = tokio::runtime::Handle::current();
        thread::spawn(move || {
            handle.block_on(async move {
                let _ = ns.delete(&name, &DeleteParams::background()).await;
                s.wait();
            });
        });

        stopped.wait();
    }
}

fn get_namespace(name: String) -> Namespace {
    Namespace {
        metadata: ObjectMeta {
            name: Some(name),
            ..Default::default()
        },
        ..Default::default()
    }
}
