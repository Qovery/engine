use serde_json::json;
use tera::{Context, Tera};

const HTTP_TEMPLATE: &str = include_str!(
    "../lib/common/charts/q-ingress-tls/templates/gateway-http-route-envoy-backend-traffic-policy.j2.yaml"
);
const GRPC_TEMPLATE: &str = include_str!(
    "../lib/common/charts/q-ingress-tls/templates/gateway-grpc-route-envoy-backend-traffic-policy.j2.yaml"
);

fn base_advanced_settings(
    service_request_timeout: Option<u32>,
    service_idle_timeout: Option<u32>,
) -> serde_json::Value {
    json!({
        "network_gateway_api_sticky_session_enable": false,
        "network_gateway_api_route_limit_rpm": null,
        "network_gateway_api_route_limit_rps": null,
        "network_gateway_api_route_limit_source_cidrs": "",
        "network_gateway_api_route_limit_headers": "",
        "network_gateway_api_custom_http_errors": null,
        "network_gateway_api_circuit_breaker_max_connections": null,
        "network_gateway_api_circuit_breaker_max_pending_requests": null,
        "network_gateway_api_circuit_breaker_max_parallel_requests": null,
        "network_gateway_api_http_request_timeout_seconds": service_request_timeout,
        "network_gateway_api_http_connection_idle_timeout_seconds": service_idle_timeout,
        "network_gateway_api_tcp_keepalive_idle_time_seconds": null,
        "network_gateway_api_tcp_keepalive_interval_seconds": null
    })
}

fn render_http_policy(
    service_request_timeout: Option<u32>,
    service_idle_timeout: Option<u32>,
    cluster_send_timeout: Option<u32>,
    cluster_read_timeout: Option<u32>,
) -> String {
    let mut tera = Tera::default();
    tera.add_raw_template("template", HTTP_TEMPLATE)
        .expect("HTTP template should parse");

    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert("http_hosts_per_namespace", &json!({"app-ns": [{"domain_name": "example.com"}]}));
    context.insert("sanitized_name", &"router-name");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("labels_group", &json!({ "common": {} }));
    context.insert(
        "advanced_settings",
        &base_advanced_settings(service_request_timeout, service_idle_timeout),
    );
    context.insert("cluster_envoy_gateway_api_http_request_timeout_seconds", &cluster_send_timeout);
    context.insert(
        "cluster_envoy_gateway_api_http_connection_idle_timeout_seconds",
        &cluster_read_timeout,
    );

    tera.render("template", &context).expect("HTTP template should render")
}

fn render_grpc_policy(
    service_request_timeout: Option<u32>,
    service_idle_timeout: Option<u32>,
    cluster_send_timeout: Option<u32>,
    cluster_read_timeout: Option<u32>,
) -> String {
    let mut tera = Tera::default();
    tera.add_raw_template("template", GRPC_TEMPLATE)
        .expect("gRPC template should parse");

    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert("grpc_hosts_per_namespace", &json!({"app-ns": [{"domain_name": "example.com"}]}));
    context.insert("sanitized_name", &"router-name");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("labels_group", &json!({ "common": {} }));
    context.insert(
        "advanced_settings",
        &base_advanced_settings(service_request_timeout, service_idle_timeout),
    );
    context.insert("cluster_envoy_gateway_api_http_request_timeout_seconds", &cluster_send_timeout);
    context.insert(
        "cluster_envoy_gateway_api_http_connection_idle_timeout_seconds",
        &cluster_read_timeout,
    );

    tera.render("template", &context).expect("gRPC template should render")
}

#[test]
fn http_policy_uses_cluster_defaults_when_service_timeout_is_missing() {
    let rendered = render_http_policy(None, None, Some(42), Some(120));
    assert!(rendered.contains("requestTimeout: 42s"));
    assert!(rendered.contains("connectionIdleTimeout: 120s"));
}

#[test]
fn http_policy_prioritizes_service_timeout_over_cluster_default() {
    let rendered = render_http_policy(Some(90), None, Some(42), Some(120));
    assert!(rendered.contains("requestTimeout: 90s"));
    assert!(!rendered.contains("requestTimeout: 42s"));
    assert!(rendered.contains("connectionIdleTimeout: 120s"));
}

#[test]
fn http_policy_prioritizes_service_idle_timeout_over_cluster_default() {
    let rendered = render_http_policy(None, Some(121), Some(42), Some(120));
    assert!(rendered.contains("requestTimeout: 42s"));
    assert!(rendered.contains("connectionIdleTimeout: 121s"));
    assert!(!rendered.contains("connectionIdleTimeout: 120s"));
}

#[test]
fn http_policy_omits_timeout_when_no_value_is_provided() {
    let rendered = render_http_policy(None, None, None, None);
    assert!(!rendered.contains("requestTimeout:"));
    assert!(!rendered.contains("connectionIdleTimeout:"));
}

#[test]
fn grpc_policy_uses_cluster_defaults_and_service_override() {
    let rendered = render_grpc_policy(Some(75), Some(121), Some(42), Some(120));
    assert!(rendered.contains("requestTimeout: 75s"));
    assert!(rendered.contains("connectionIdleTimeout: 121s"));
}
