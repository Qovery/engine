//! Covers the ClientTrafficPolicy used to retain HTTP/2 for a router's shared TLS certificate.

use qovery_engine::tera_utils::render_one_off;
use serde::Deserialize;
use serde_json::json;
use serde_yaml::Value as YamlValue;
use std::process::Command;
use tera::Context;

const CLIENT_TRAFFIC_POLICY_TEMPLATE: &str =
    include_str!("fixtures/pre_escaping/q-ingress-tls/gateway-client-traffic-policy.j2.yaml");
const LISTENER_SET_TEMPLATE: &str = include_str!("fixtures/pre_escaping/q-ingress-tls/listenerset.j2.yaml");
const CLUSTER_GATEWAY_CHART_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/lib/common/bootstrap/charts/qovery-cluster-gateway"
);

fn render_policy(
    certificate_alternative_names: serde_json::Value,
    deploy_listenerset: bool,
    client_traffic_policy_supports_listenerset: bool,
    has_https_listeners: bool,
    router_policy_http2_enabled: bool,
) -> String {
    render_policy_with_cluster_settings(
        certificate_alternative_names,
        deploy_listenerset,
        client_traffic_policy_supports_listenerset,
        has_https_listeners,
        json!({
            "trusted_hops": null,
            "trusted_cidrs": [],
            "client_validation_certificates": [],
            "stream_idle_timeout_seconds": null,
            "path_disable_merge_slashes": false,
            "path_escaped_slashes_action": "UnescapeAndRedirect",
            "proxy_protocol_enabled": false,
            "use_v1beta1_reference_grant": false,
        }),
        router_policy_http2_enabled,
    )
}

fn render_policy_with_cluster_settings(
    certificate_alternative_names: serde_json::Value,
    deploy_listenerset: bool,
    client_traffic_policy_supports_listenerset: bool,
    has_https_listeners: bool,
    cluster_settings: serde_json::Value,
    router_policy_http2_enabled: bool,
) -> String {
    render_policy_with_gateway_hosts(
        certificate_alternative_names,
        deploy_listenerset,
        client_traffic_policy_supports_listenerset,
        if has_https_listeners {
            json!({ "app-ns": [{}] })
        } else {
            json!({})
        },
        cluster_settings,
        router_policy_http2_enabled,
    )
}

fn render_policy_with_gateway_hosts(
    certificate_alternative_names: serde_json::Value,
    deploy_listenerset: bool,
    client_traffic_policy_supports_listenerset: bool,
    http_hosts_per_namespace_gateway: serde_json::Value,
    cluster_settings: serde_json::Value,
    router_policy_http2_enabled: bool,
) -> String {
    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert("k8s_deploy_listenerset", &deploy_listenerset);
    context.insert(
        "k8s_deploy_client_traffic_policy_listenerset",
        &client_traffic_policy_supports_listenerset,
    );
    context.insert("http_hosts_per_namespace_gateway", &http_hosts_per_namespace_gateway);
    context.insert("certificate_alternative_names", &certificate_alternative_names);
    context.insert("sanitized_name", &"api-router");
    context.insert("id", &"router-id");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("labels_group", &json!({ "common": {} }));
    context.insert("router_policy_http2_enabled", &router_policy_http2_enabled);
    context.insert(
        "cluster_envoy_client_ip_detection_x_forwarded_for_number_trusted_hops",
        &cluster_settings["trusted_hops"],
    );
    context.insert(
        "cluster_envoy_client_ip_detection_x_forwarded_for_trusted_cidrs",
        &cluster_settings["trusted_cidrs"],
    );
    context.insert(
        "cluster_envoy_client_validation_ca_certificates",
        &cluster_settings["client_validation_certificates"],
    );
    context.insert(
        "cluster_envoy_gateway_api_http_stream_idle_timeout_seconds",
        &cluster_settings["stream_idle_timeout_seconds"],
    );
    context.insert(
        "cluster_envoy_gateway_api_path_disable_merge_slashes",
        &cluster_settings["path_disable_merge_slashes"],
    );
    context.insert(
        "cluster_envoy_gateway_api_path_escaped_slashes_action",
        &cluster_settings["path_escaped_slashes_action"],
    );
    context.insert(
        "cluster_envoy_proxy_protocol_enabled",
        &cluster_settings["proxy_protocol_enabled"],
    );
    context.insert(
        "k8s_use_v1beta1_reference_grant",
        &cluster_settings["use_v1beta1_reference_grant"],
    );

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

fn render_cluster_gateway_client_traffic_policy() -> serde_json::Value {
    let output = Command::new("helm")
        .args([
            "template",
            "listener-policy-contract-test",
            CLUSTER_GATEWAY_CHART_PATH,
            "--namespace",
            "qovery",
            "--set",
            "gateway.qoveryPublic.enableProxyProtocol=true",
            "--set",
            "gateway.qoveryPublic.proxyProtocol.optional=true",
            "--set",
            "gateway.qoveryPublic.xForwardedFor.numberTrustedHops=0",
            "--set",
            "gateway.qoveryPublic.timeout.http.streamIdleTimeoutSeconds=300",
            "--set",
            "gateway.qoveryPublic.path.disableMergeSlashes=true",
            "--set",
            "gateway.qoveryPublic.path.escapedSlashesAction=KeepUnchanged",
            "--set",
            "gateway.qoveryPublic.clientValidation.caCertificates[0].name=envoy-client-validation-origin-pull-ca",
            "--set",
            "gateway.qoveryPublic.clientValidation.caCertificates[0].namespace=qovery",
            "--set-string",
            "gateway.qoveryPublic.clientValidation.caCertificates[0].caCrt=test-ca",
        ])
        .output()
        .expect("helm must be available to render the Qovery cluster gateway chart");

    assert!(
        output.status.success(),
        "cluster gateway chart rendering failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_yaml::Deserializer::from_slice(&output.stdout)
        .map(|document| YamlValue::deserialize(document).expect("rendered chart must contain valid YAML"))
        .filter(|document| !document.is_null())
        .map(|document| serde_json::to_value(document).expect("YAML must convert to JSON"))
        .find(|manifest: &serde_json::Value| {
            manifest["kind"] == "ClientTrafficPolicy"
                && manifest["metadata"]["name"] == "qovery-cluster-public-gateway-client-traffic-policy"
        })
        .expect("cluster gateway chart must render its ClientTrafficPolicy")
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
fn router_policy_uses_http1_by_default() {
    let rendered = render_policy(
        json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
        true,
        true,
        true,
        false,
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
        policy["spec"]["headers"]["withUnderscoresAction"].as_str(),
        Some("DropHeader"),
        "the ListenerSet policy replaces the Gateway policy, so it must preserve underscored-header handling"
    );
    assert_eq!(policy["spec"]["headers"]["enableEnvoyHeaders"].as_bool(), Some(true));
    assert_eq!(policy["spec"]["path"]["disableMergeSlashes"].as_bool(), Some(false));
    assert_eq!(
        policy["spec"]["path"]["escapedSlashesAction"].as_str(),
        Some("UnescapeAndRedirect")
    );
    let alpn_protocols = policy["spec"]["tls"]["alpnProtocols"]
        .as_sequence()
        .expect("the router policy must define ALPN protocols");

    assert_eq!(alpn_protocols.len(), 1);
    assert_eq!(alpn_protocols[0].as_str(), Some("http/1.1"));
}

#[test]
fn router_policy_keeps_http2_enabled() {
    let rendered = render_policy(
        json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
        true,
        true,
        true,
        true,
    );
    let policy = manifests(&rendered)
        .into_iter()
        .next()
        .expect("Hyperline's multi-SAN ListenerSet must receive a ClientTrafficPolicy");

    let alpn_protocols = policy["spec"]["tls"]["alpnProtocols"]
        .as_sequence()
        .expect("the router policy must define ALPN protocols");

    assert_eq!(alpn_protocols.len(), 2);
    assert_eq!(alpn_protocols[0].as_str(), Some("h2"));
    assert_eq!(alpn_protocols[1].as_str(), Some("http/1.1"));
}

#[test]
fn multi_san_listenerset_keeps_configured_gateway_traffic_settings() {
    let rendered = render_policy_with_cluster_settings(
        json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
        true,
        true,
        true,
        json!({
            "trusted_hops": 0,
            "trusted_cidrs": [],
            "client_validation_certificates": [{
                "name": "envoy-client-validation-origin-pull-ca",
                "namespace": "qovery"
            }],
            "stream_idle_timeout_seconds": 300,
            "path_disable_merge_slashes": true,
            "path_escaped_slashes_action": "KeepUnchanged",
            "proxy_protocol_enabled": true,
            "use_v1beta1_reference_grant": true,
        }),
        false,
    );
    let policies = manifests(&rendered);
    let policy = policies
        .iter()
        .find(|manifest| manifest["kind"].as_str() == Some("ClientTrafficPolicy"))
        .expect("the ListenerSet ClientTrafficPolicy must be rendered");
    let reference_grant = policies
        .iter()
        .find(|manifest| manifest["kind"].as_str() == Some("ReferenceGrant"))
        .expect("cross-namespace client-validation Secret requires a ReferenceGrant");

    assert_eq!(
        policy["spec"]["clientIPDetection"]["xForwardedFor"]["numTrustedHops"].as_i64(),
        Some(0)
    );
    assert_eq!(policy["spec"]["timeout"]["http"]["streamIdleTimeout"].as_str(), Some("300s"));
    assert_eq!(policy["spec"]["path"]["disableMergeSlashes"].as_bool(), Some(true));
    assert_eq!(policy["spec"]["path"]["escapedSlashesAction"].as_str(), Some("KeepUnchanged"));
    assert_eq!(policy["spec"]["proxyProtocol"]["optional"].as_bool(), Some(true));
    assert_eq!(
        policy["spec"]["tls"]["clientValidation"]["caCertificateRefs"][0]["name"].as_str(),
        Some("envoy-client-validation-origin-pull-ca")
    );
    assert_eq!(
        reference_grant["apiVersion"].as_str(),
        Some("gateway.networking.k8s.io/v1beta1")
    );
    assert_eq!(reference_grant["metadata"]["namespace"].as_str(), Some("qovery"));
    assert_eq!(reference_grant["spec"]["from"][0]["namespace"].as_str(), Some("app-ns"));
    assert_eq!(
        reference_grant["spec"]["to"][0]["name"].as_str(),
        Some("envoy-client-validation-origin-pull-ca")
    );
}

#[test]
fn client_validation_reference_grants_are_unique_per_router_namespace() {
    let rendered = render_policy_with_gateway_hosts(
        json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
        true,
        true,
        json!({
            "app-ns": [{}],
            "other-app-ns": [{}]
        }),
        json!({
            "trusted_hops": null,
            "trusted_cidrs": [],
            "client_validation_certificates": [{
                "name": "envoy-client-validation-origin-pull-ca",
                "namespace": "qovery"
            }],
            "stream_idle_timeout_seconds": null,
            "path_disable_merge_slashes": false,
            "path_escaped_slashes_action": "UnescapeAndRedirect",
            "proxy_protocol_enabled": false,
            "use_v1beta1_reference_grant": false,
        }),
        false,
    );
    let grants = manifests(&rendered)
        .into_iter()
        .filter(|manifest| manifest["kind"].as_str() == Some("ReferenceGrant"))
        .collect::<Vec<_>>();

    assert_eq!(grants.len(), 2, "each router namespace must receive its own ReferenceGrant");
    for (router_namespace, grant_name) in [
        ("app-ns", "router-router-id-client-validation-0-app-ns"),
        ("other-app-ns", "router-router-id-client-validation-0-other-app-ns"),
    ] {
        let grant = grants
            .iter()
            .find(|grant| grant["spec"]["from"][0]["namespace"].as_str() == Some(router_namespace))
            .expect("a ReferenceGrant must exist for every router namespace");

        assert_eq!(grant["metadata"]["name"].as_str(), Some(grant_name));
        assert_eq!(grant["metadata"]["namespace"].as_str(), Some("qovery"));
    }
}

#[test]
fn multi_san_listenerset_policy_matches_the_complete_shared_policy_contract() {
    let rendered = render_policy_with_cluster_settings(
        json!([
            { "domain": "api.example.com" },
            { "domain": "alias.example.com" }
        ]),
        true,
        true,
        true,
        json!({
            "trusted_hops": 0,
            "trusted_cidrs": [],
            "client_validation_certificates": [{
                "name": "envoy-client-validation-origin-pull-ca",
                "namespace": "qovery"
            }],
            "stream_idle_timeout_seconds": 300,
            "path_disable_merge_slashes": true,
            "path_escaped_slashes_action": "KeepUnchanged",
            "proxy_protocol_enabled": true,
            "use_v1beta1_reference_grant": false,
        }),
        false,
    );
    let listener_set_policy = manifests(&rendered)
        .into_iter()
        .find(|manifest| manifest["kind"].as_str() == Some("ClientTrafficPolicy"))
        .expect("the ListenerSet ClientTrafficPolicy must be rendered");
    let mut expected_spec = render_cluster_gateway_client_traffic_policy()["spec"].clone();

    expected_spec["targetRefs"] = serde_json::to_value(&listener_set_policy["spec"]["targetRefs"])
        .expect("ListenerSet target references must convert to JSON");
    expected_spec["tls"]["alpnProtocols"] = json!(["http/1.1"]);

    assert_eq!(
        serde_json::to_value(&listener_set_policy["spec"]).expect("ListenerSet policy spec must convert to JSON"),
        expected_spec,
        "Envoy Gateway selects the ListenerSet policy instead of merging it with the Gateway policy; every shared setting must stay in sync"
    );
}

#[test]
fn policy_is_absent_without_an_overlapping_listenerset_certificate() {
    assert!(
        manifests(&render_policy(
            json!([{ "domain": "api.example.com" }]),
            true,
            true,
            true,
            false
        ))
        .is_empty(),
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
            false,
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
        false,
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
        false,
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
