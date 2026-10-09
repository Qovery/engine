use kube::Api;
use kube::api::{ApiResource, ListParams};
use qovery_engine::cmd::kubectl::kubectl_client_traffic_policy_supports_listenerset;
use qovery_engine::runtime::block_on;
use retry::delay::Fibonacci;
use serde_json::json;
use uuid::Uuid;

// Qovery overrides Envoy Gateway's upstream controller name in the bootstrap chart.
// Policy status is written by that configured controller, not by Envoy's default name.
const ENVOY_GATEWAY_CONTROLLER_NAME: &str = "qovery.com/gateway-controller";

/// Asserts that a multi-SAN router's ListenerSet received an accepted ALPN policy.
/// `http2_enabled` must match whether the router's org is on the HTTP/2 allowlist.
pub fn assert_multi_san_client_traffic_policies(
    kube_client: kube::Client,
    router_namespace: &str,
    router_id: Uuid,
    router_name: &str,
    http2_enabled: bool,
) {
    if !kubectl_client_traffic_policy_supports_listenerset(&kube_client) {
        return;
    }

    let expected_alpn_protocols = if http2_enabled {
        json!(["h2", "http/1.1"])
    } else {
        json!(["http/1.1"])
    };

    let api_resource = ApiResource {
        group: "gateway.envoyproxy.io".to_string(),
        version: "v1alpha1".to_string(),
        kind: "ClientTrafficPolicy".to_string(),
        api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
        plural: "clienttrafficpolicies".to_string(),
    };
    let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, router_namespace, &api_resource);
    let router_id_label = router_id.to_string();

    let policies = retry::retry(Fibonacci::from_millis(3000).take(10), || {
        let policies = match block_on(api.list(&ListParams::default())) {
            Ok(policies) => policies,
            Err(error) => {
                return Err(retry::OperationResult::<Vec<kube::core::DynamicObject>, String>::Retry(
                    format!("failed to list ClientTrafficPolicies: {error}"),
                ));
            }
        };
        let router_policies: Vec<_> = policies
            .items
            .into_iter()
            .filter(|policy| {
                policy
                    .metadata
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.get("qovery.com/service-id"))
                    .is_some_and(|service_id| service_id == &router_id_label)
            })
            .collect();

        let accepted_count = router_policies.iter().filter(|policy| is_accepted_by_envoy_gateway(policy)).count();
        if router_policies.len() != 1 || accepted_count != 1 {
            return Err(retry::OperationResult::Retry(format!(
                "expected one accepted ClientTrafficPolicy for router {router_id}, found {} total ({accepted_count} accepted)",
                router_policies.len(),
            )));
        }

        Ok(router_policies)
    })
    .unwrap_or_else(|error| panic!("ClientTrafficPolicies did not become accepted: {error:?}"));

    for policy in policies {
        let expected_listener_set_name = format!("{router_name}-listeners");

        assert_eq!(
            policy.data["spec"]["targetRefs"][0]["group"].as_str(),
            Some("gateway.networking.k8s.io")
        );
        assert_eq!(policy.data["spec"]["targetRefs"][0]["kind"].as_str(), Some("ListenerSet"));
        assert_eq!(
            policy.data["spec"]["targetRefs"][0]["name"].as_str(),
            Some(expected_listener_set_name.as_str())
        );
        assert!(policy.data["spec"]["targetRefs"][0].get("sectionName").is_none());
        assert_eq!(policy.data["spec"]["tls"]["alpnProtocols"], expected_alpn_protocols);
    }
}

fn is_accepted_by_envoy_gateway(policy: &kube::core::DynamicObject) -> bool {
    policy.data["status"]["ancestors"].as_array().is_some_and(|ancestors| {
        ancestors.iter().any(|ancestor| {
            ancestor["controllerName"].as_str() == Some(ENVOY_GATEWAY_CONTROLLER_NAME)
                && ancestor["conditions"].as_array().is_some_and(|conditions| {
                    conditions.iter().any(|condition| {
                        condition["type"].as_str() == Some("Accepted") && condition["status"].as_str() == Some("True")
                    })
                })
        })
    })
}
