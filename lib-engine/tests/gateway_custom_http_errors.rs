//! Covers the `responseOverride` block of the gateway BackendTrafficPolicy, whose HTTP error
//! codes are typed `Option<Vec<u16>>` but reach the template as the comma-separated
//! string their serializer emits (`io_models::types::http_status_codes`).

#[path = "helpers/native_helm.rs"]
mod native_helm;

use native_helm::{render_per_hostname_router_template, render_router_template};
use serde_json::json;
use tera::Context;

const HTTP_POLICY: &str =
    include_str!("../lib/common/charts/q-ingress-tls/templates/gateway-http-route-envoy-backend-traffic-policy.yaml");
const PER_HOSTNAME_HTTP_POLICY: &str = include_str!(
    "../lib/common/charts/q-ingress-tls-per-hostname/templates/gateway-http-route-envoy-backend-traffic-policy.yaml"
);

fn render_with_custom_http_errors(
    template: &str,
    render_template: fn(&str, &Context) -> Result<String, tera::Error>,
    service_custom_http_errors: serde_json::Value,
    cluster_custom_http_errors: serde_json::Value,
) -> String {
    let gateway_custom_http_error_pages = service_custom_http_errors
        .as_str()
        .map(|status_codes| {
            status_codes
                .split(',')
                .map(str::trim)
                .map(|status_code| {
                    json!({
                        "status_code": status_code.parse::<u16>().expect("test status code must be valid"),
                        "response_body": "test response body",
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert("gateway_http_routes_per_namespace", &json!({"app-ns": [{}]}));
    context.insert("sanitized_name", &"router-name");
    context.insert("long_id", &"00000000-0000-0000-0000-000000000001");
    context.insert("associated_service_long_id", &"00000000-0000-0000-0000-000000000002");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"00000000-0000-0000-0000-000000000003");
    context.insert("project_long_id", &"00000000-0000-0000-0000-000000000004");
    context.insert("labels_group", &json!({ "common": {} }));
    context.insert("cluster_envoy_gateway_api_http_request_timeout_seconds", &json!(null));
    context.insert("cluster_envoy_gateway_api_http_connection_idle_timeout_seconds", &json!(null));
    context.insert("cluster_envoy_gateway_api_http_max_stream_duration_seconds", &json!(null));
    context.insert("cluster_envoy_enable_compression", &false);
    context.insert("cluster_envoy_custom_http_errors_default", &cluster_custom_http_errors);
    context.insert("gateway_custom_http_error_pages", &gateway_custom_http_error_pages);
    context.insert("resolved_gateway_api_retry_num_retries", &json!(null));
    context.insert("resolved_gateway_api_retry_per_try_timeout_seconds", &json!(null));
    context.insert("resolved_gateway_api_retry_retry_on_triggers", &json!([]));
    context.insert("resolved_gateway_api_retry_http_status_codes", &json!([]));
    context.insert(
        "advanced_settings",
        &json!({
            "network_gateway_api_custom_http_errors": service_custom_http_errors,
            "network_gateway_api_sticky_session_enable": false,
            "network_gateway_api_route_limit_rpm": null,
            "network_gateway_api_route_limit_rps": null,
            "network_gateway_api_route_limit_source_cidrs": "",
            "network_gateway_api_route_limit_headers": "",
            "network_gateway_api_circuit_breaker_max_connections": null,
            "network_gateway_api_circuit_breaker_max_pending_requests": null,
            "network_gateway_api_circuit_breaker_max_parallel_requests": null,
            "network_gateway_api_http_request_timeout_seconds": null,
            "network_gateway_api_http_connection_idle_timeout_seconds": null,
            "network_gateway_api_http_max_stream_duration_seconds": null,
            "network_gateway_api_tcp_keepalive_idle_time_seconds": null,
            "network_gateway_api_tcp_keepalive_interval_seconds": null,
        }),
    );

    render_template(template, &context).expect("policy template should render")
}

#[test]
fn custom_http_errors_render_one_override_per_code() {
    for (template, render_template) in [
        (
            HTTP_POLICY,
            render_router_template as fn(&str, &Context) -> Result<String, tera::Error>,
        ),
        (PER_HOSTNAME_HTTP_POLICY, render_per_hostname_router_template),
    ] {
        let rendered = render_with_custom_http_errors(template, render_template, json!("404,503"), json!(null));
        let policy: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("policy must parse as YAML");
        let overrides = policy["spec"]["responseOverride"]
            .as_sequence()
            .expect("responseOverride must be a sequence");

        assert_eq!(overrides.len(), 2, "one override per code in:\n{rendered}");
        // The CRD field is an integer, and each status code receives its own ConfigMap.
        assert_eq!(overrides[0]["match"]["statusCodes"][0]["value"].as_u64(), Some(404));
        assert_eq!(
            overrides[0]["response"]["body"]["valueRef"]["name"].as_str(),
            Some("router-name-error-pages-404")
        );
        assert_eq!(
            overrides[0]["response"]["body"]["valueRef"]["key"].as_str(),
            Some("response.body")
        );
        assert_eq!(overrides[1]["match"]["statusCodes"][0]["value"].as_u64(), Some(503));
        assert_eq!(
            overrides[1]["response"]["body"]["valueRef"]["name"].as_str(),
            Some("router-name-error-pages-503")
        );
        assert_eq!(
            overrides[1]["response"]["body"]["valueRef"]["key"].as_str(),
            Some("response.body")
        );
    }
}

#[test]
fn custom_http_errors_are_absent_when_unset() {
    let rendered = render_with_custom_http_errors(HTTP_POLICY, render_router_template, json!(null), json!(null));
    let policy: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("policy must parse as YAML");

    assert!(
        policy["spec"]["responseOverride"].is_null(),
        "no override block without codes in:\n{rendered}"
    );
}

#[test]
fn cluster_default_custom_http_errors_render_when_service_override_is_unset() {
    let rendered = render_with_custom_http_errors(HTTP_POLICY, render_router_template, json!(null), json!([404, 503]));
    let policy: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("policy must parse as YAML");
    let overrides = policy["spec"]["responseOverride"]
        .as_sequence()
        .expect("cluster default responseOverride must be a sequence");

    assert_eq!(overrides.len(), 2, "one override per cluster default code in:\n{rendered}");
    assert_eq!(overrides[0]["match"]["statusCodes"][0]["value"].as_u64(), Some(404));
    assert_eq!(
        overrides[0]["response"]["body"]["valueRef"]["name"].as_str(),
        Some("qovery-cluster-gateway-default-error-pages")
    );
    assert_eq!(overrides[1]["response"]["body"]["valueRef"]["key"].as_str(), Some("503.html"));
}

#[test]
fn service_custom_http_errors_override_cluster_defaults() {
    let rendered = render_with_custom_http_errors(HTTP_POLICY, render_router_template, json!("418"), json!([404, 503]));
    let policy: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("policy must parse as YAML");
    let overrides = policy["spec"]["responseOverride"]
        .as_sequence()
        .expect("service responseOverride must be a sequence");

    assert_eq!(
        overrides.len(),
        1,
        "service override must replace cluster defaults in:\n{rendered}"
    );
    assert_eq!(overrides[0]["match"]["statusCodes"][0]["value"].as_u64(), Some(418));
    assert_eq!(
        overrides[0]["response"]["body"]["valueRef"]["name"].as_str(),
        Some("router-name-error-pages-418")
    );
}
