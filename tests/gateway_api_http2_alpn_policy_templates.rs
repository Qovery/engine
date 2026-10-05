//! Covers the ClientTrafficPolicy used to retain HTTP/2 for a router's shared TLS certificate.

use qovery_engine::tera_utils::render_one_off;
use serde_json::json;
use tera::Context;

const CLIENT_TRAFFIC_POLICY_TEMPLATE: &str =
    include_str!("fixtures/pre_escaping/q-ingress-tls/gateway-client-traffic-policy.j2.yaml");
const LISTENER_SET_TEMPLATE: &str = include_str!("fixtures/pre_escaping/q-ingress-tls/listenerset.j2.yaml");

fn render_policy(
    certificate_alternative_names: serde_json::Value,
    deploy_listenerset: bool,
    client_traffic_policy_supports_listenerset: bool,
    has_https_listeners: bool,
) -> String {
    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert("k8s_deploy_listenerset", &deploy_listenerset);
    context.insert(
        "k8s_deploy_client_traffic_policy_listenerset",
        &client_traffic_policy_supports_listenerset,
    );
    context.insert(
        "http_hosts_per_namespace_gateway",
        &if has_https_listeners {
            json!({ "app-ns": [{}] })
        } else {
            json!({})
        },
    );
    context.insert("certificate_alternative_names", &certificate_alternative_names);
    context.insert("sanitized_name", &"api-router");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("labels_group", &json!({ "common": {} }));

    render_one_off(CLIENT_TRAFFIC_POLICY_TEMPLATE, &context).expect("ClientTrafficPolicy template should render")
}

fn manifests(rendered: &str) -> Vec<serde_yaml::Value> {
    rendered
        .split("\n---")
        .filter_map(|document| {
            let document = document.trim();
            (!document.is_empty())
                .then(|| serde_yaml::from_str(document).expect("rendered ClientTrafficPolicy must parse as YAML"))
        })
        .collect()
}

fn render_listener_set(deploy_listenerset: bool) -> String {
    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert("k8s_deploy_listenerset", &deploy_listenerset);
    context.insert("http_hosts_per_namespace_gateway", &json!({ "app-ns": [{}] }));
    context.insert(
        "certificate_alternative_names",
        &json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
    );
    context.insert("sanitized_name", &"api-router");
    context.insert("id", &"router-id");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("labels_group", &json!({ "common": {} }));

    render_one_off(LISTENER_SET_TEMPLATE, &context).expect("ListenerSet template should render")
}

#[test]
fn multi_san_listenerset_gets_a_router_scoped_http2_alpn_policy() {
    let rendered = render_policy(
        json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
        true,
        true,
        true,
    );
    let policies = manifests(&rendered);

    assert_eq!(policies.len(), 1, "one policy is required for each ListenerSet:\n{rendered}");
    let policy = &policies[0];
    assert_eq!(policy["apiVersion"].as_str(), Some("gateway.envoyproxy.io/v1alpha1"));
    assert_eq!(policy["kind"].as_str(), Some("ClientTrafficPolicy"));
    assert_eq!(policy["metadata"]["namespace"].as_str(), Some("app-ns"));
    assert_eq!(
        policy["spec"]["targetRefs"][0]["group"].as_str(),
        Some("gateway.networking.k8s.io")
    );
    assert_eq!(policy["spec"]["targetRefs"][0]["kind"].as_str(), Some("ListenerSet"));
    assert_eq!(policy["spec"]["targetRefs"][0]["name"].as_str(), Some("api-router-listeners"));
    assert!(policy["spec"]["targetRefs"][0].get("sectionName").is_none());
    assert_eq!(
        policy["spec"]["tls"]["alpnProtocols"][0].as_str(),
        Some("h2"),
        "policy must explicitly override Envoy Gateway's HTTP/1.1 overlap fallback"
    );
    assert_eq!(policy["spec"]["tls"]["alpnProtocols"][1].as_str(), Some("http/1.1"));
}

#[test]
fn policy_is_absent_without_an_overlapping_listenerset_certificate() {
    assert!(
        manifests(&render_policy(json!([{ "domain": "api.example.com" }]), true, true, true)).is_empty(),
        "a single-SAN certificate already retains Envoy Gateway's default HTTP/2 ALPN"
    );
    assert!(
        manifests(&render_policy(
            json!([
                { "domain": "api.example.com" },
                { "domain": "alias.example.com" }
            ]),
            false,
            true,
            true,
        ))
        .is_empty(),
        "the policy must not target a ListenerSet section that was not deployed"
    );
    assert!(
        manifests(&render_policy(
            json!([
                { "domain": "api.example.com" },
                { "domain": "alias.example.com" }
            ]),
            true,
            true,
            false,
        ))
        .is_empty(),
        "the policy must not target HTTPS ListenerSet sections that were not emitted"
    );
}

#[test]
fn policy_is_absent_when_the_cluster_only_accepts_gateway_targets() {
    let rendered = render_policy(
        json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
        true,
        false,
        true,
    );

    assert!(
        manifests(&rendered).is_empty(),
        "clusters whose ClientTrafficPolicy CRD only allows Gateway targets must not receive a ListenerSet policy"
    );
}

#[test]
fn gke_with_an_older_client_traffic_policy_crd_keeps_its_listenerset() {
    let policy = render_policy(
        json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
        true,
        false,
        true,
    );
    let listener_sets = manifests(&render_listener_set(true));

    assert!(
        manifests(&policy).is_empty(),
        "an older ClientTrafficPolicy CRD must not receive the unsupported ListenerSet policy"
    );
    assert_eq!(
        listener_sets.len(),
        1,
        "GKE must retain the ListenerSet that routes the router TLS certificate"
    );
    assert_eq!(listener_sets[0]["kind"].as_str(), Some("ListenerSet"));
    assert_eq!(listener_sets[0]["metadata"]["namespace"].as_str(), Some("app-ns"));
}
