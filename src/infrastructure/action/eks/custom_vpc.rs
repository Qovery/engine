use k8s_openapi::api::apps::v1::DaemonSet;
use kube::Api;
use kube::api::{Patch, PatchParams};

pub async fn patch_kube_proxy_for_aws_user_network(kube_client: kube::Client) -> Result<DaemonSet, kube::Error> {
    let daemon_set: Api<DaemonSet> = Api::namespaced(kube_client, "kube-system");
    let patch_params = PatchParams::default();
    let daemonset_patch = serde_json::json!({
      "spec": {
        "template": {
          "spec": {
            "containers": [
              {
                "name": "kube-proxy",
                "command": [
                  "kube-proxy",
                  "--v=2",
                  "--hostname-override=$(NODE_NAME)",
                  "--config=/var/lib/kube-proxy-config/config"
                ],
                "env": [
                  {
                    "name": "NODE_NAME",
                    "valueFrom": {
                      "fieldRef": {
                        "apiVersion": "v1",
                        "fieldPath": "spec.nodeName"
                      }
                    }
                  }
                ]
              }
            ]
          }
        }
      }
    });

    daemon_set
        .patch("kube-proxy", &patch_params, &Patch::Strategic(daemonset_patch))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[ignore]
    #[tokio::test]
    async fn test_kube_proxy_patch() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = kube::Client::try_default().await.unwrap();
        patch_kube_proxy_for_aws_user_network(kube_client).await?;

        Ok(())
    }
}
