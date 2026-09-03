//! Covers the `responseOverride` block of the gateway BackendTrafficPolicy, whose HTTP error
//! codes are typed `Option<Vec<u16>>` but reach the template as the comma-separated
//! string their serializer emits (`io_models::types::http_status_codes`).

use qovery_engine::tera_utils::render_one_off;
use serde_json::json;
use tera::Context;

const HTTP_POLICY: &str = include_str!(
    "../lib/common/charts/q-ingress-tls/templates/gateway-http-route-envoy-backend-traffic-policy.j2.yaml"
);

fn render_with_custom_http_errors(custom_http_errors: serde_json::Value) -> String {
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
    context.insert("resolved_gateway_api_retry_num_retries", &json!(null));
    context.insert("resolved_gateway_api_retry_per_try_timeout_seconds", &json!(null));
    context.insert("resolved_gateway_api_retry_retry_on_triggers", &json!([]));
    context.insert("resolved_gateway_api_retry_http_status_codes", &json!([]));
    context.insert(
        "advanced_settings",
        &json!({
            "network_gateway_api_custom_http_errors": custom_http_errors,
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

    render_one_off(HTTP_POLICY, &context).expect("policy template should render")
}

#[test]
fn custom_http_errors_render_one_override_per_code() {
    let rendered = render_with_custom_http_errors(json!("404,503"));
    let policy: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("policy must parse as YAML");
    let overrides = policy["spec"]["responseOverride"]
        .as_sequence()
        .expect("responseOverride must be a sequence");

    assert_eq!(overrides.len(), 2, "one override per code in:\n{rendered}");
    // The CRD field is an integer, and the ConfigMap holds a `<code>.html` key per code.
    assert_eq!(overrides[0]["match"]["statusCodes"][0]["value"].as_u64(), Some(404));
    assert_eq!(overrides[0]["response"]["body"]["valueRef"]["key"].as_str(), Some("404.html"));
    assert_eq!(overrides[1]["match"]["statusCodes"][0]["value"].as_u64(), Some(503));
}

#[test]
fn custom_http_errors_are_absent_when_unset() {
    let rendered = render_with_custom_http_errors(json!(null));
    let policy: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("policy must parse as YAML");

    assert!(
        policy["spec"]["responseOverride"].is_null(),
        "no override block without codes in:\n{rendered}"
    );
}
