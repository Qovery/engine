use qovery_engine::tera_utils::register_filters;
use serde_json::json;
use tera::{Context, Tera};

const HTTP_TEMPLATE: &str = include_str!(
    "../lib/common/charts/q-ingress-tls/templates/gateway-http-route-envoy-backend-traffic-policy.j2.yaml"
);
const GRPC_TEMPLATE: &str = include_str!(
    "../lib/common/charts/q-ingress-tls/templates/gateway-grpc-route-envoy-backend-traffic-policy.j2.yaml"
);
const HTTP_ROUTE_TEMPLATE: &str =
    include_str!("../lib/common/charts/q-ingress-tls/templates/gateway-http-route.j2.yaml");
const GRPC_ROUTE_TEMPLATE: &str =
    include_str!("../lib/common/charts/q-ingress-tls/templates/gateway-grpc-route.j2.yaml");

#[derive(Clone, Default)]
struct RetrySettings {
    num_retries: Option<u32>,
    retry_on: String,
    http_status_codes: String,
    per_try_timeout_seconds: Option<u32>,
}

#[derive(Clone, Default)]
struct ResolvedRetrySettings {
    num_retries: Option<u32>,
    retry_on_triggers: Vec<String>,
    http_status_codes: Vec<String>,
    per_try_timeout_seconds: Option<u32>,
}

fn no_retry_settings() -> RetrySettings {
    RetrySettings::default()
}

fn parse_csv_setting_to_vec(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

fn select_retry_csv_with_fallback(service_value: &str, cluster_value: &str) -> Vec<String> {
    if !service_value.trim().is_empty() {
        return parse_csv_setting_to_vec(service_value);
    }
    if !cluster_value.trim().is_empty() {
        return parse_csv_setting_to_vec(cluster_value);
    }
    Vec::new()
}

fn resolve_retry_settings(service_retry: &RetrySettings, cluster_retry: &RetrySettings) -> ResolvedRetrySettings {
    ResolvedRetrySettings {
        num_retries: service_retry.num_retries.or(cluster_retry.num_retries),
        retry_on_triggers: select_retry_csv_with_fallback(&service_retry.retry_on, &cluster_retry.retry_on),
        http_status_codes: select_retry_csv_with_fallback(
            &service_retry.http_status_codes,
            &cluster_retry.http_status_codes,
        ),
        per_try_timeout_seconds: service_retry
            .per_try_timeout_seconds
            .or(cluster_retry.per_try_timeout_seconds),
    }
}

fn base_advanced_settings(
    service_request_timeout: Option<u32>,
    service_idle_timeout: Option<u32>,
    service_max_stream_duration: Option<u32>,
    service_retry: &RetrySettings,
) -> serde_json::Value {
    json!({
        "network_gateway_api_sticky_session_enable": false,
        "network_gateway_api_route_limit_rpm": null,
        "network_gateway_api_route_limit_rps": null,
        "network_gateway_api_route_limit_source_cidrs": "",
        "network_gateway_api_route_limit_headers": "",
        "network_gateway_api_retry_num_retries": service_retry.num_retries,
        "network_gateway_api_retry_retry_on": service_retry.retry_on,
        "network_gateway_api_retry_http_status_codes": service_retry.http_status_codes,
        "network_gateway_api_retry_per_try_timeout_seconds": service_retry.per_try_timeout_seconds,
        "network_gateway_api_custom_http_errors": null,
        "network_gateway_api_circuit_breaker_max_connections": null,
        "network_gateway_api_circuit_breaker_max_pending_requests": null,
        "network_gateway_api_circuit_breaker_max_parallel_requests": null,
        "network_gateway_api_http_request_timeout_seconds": service_request_timeout,
        "network_gateway_api_http_connection_idle_timeout_seconds": service_idle_timeout,
        "network_gateway_api_http_max_stream_duration_seconds": service_max_stream_duration,
        "network_gateway_api_tcp_keepalive_idle_time_seconds": null,
        "network_gateway_api_tcp_keepalive_interval_seconds": null,
        "network_gateway_api_path_disable_merge_slashes": false,
        "network_gateway_api_path_escaped_slashes_action": "UnescapeAndRedirect"
    })
}

fn render_http_policy(
    service_request_timeout: Option<u32>,
    service_idle_timeout: Option<u32>,
    service_max_stream_duration: Option<u32>,
    cluster_send_timeout: Option<u32>,
    cluster_read_timeout: Option<u32>,
    cluster_max_stream_duration: Option<u32>,
) -> String {
    render_http_policy_with_retry(
        service_request_timeout,
        service_idle_timeout,
        service_max_stream_duration,
        cluster_send_timeout,
        cluster_read_timeout,
        cluster_max_stream_duration,
        no_retry_settings(),
        no_retry_settings(),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_http_policy_with_retry(
    service_request_timeout: Option<u32>,
    service_idle_timeout: Option<u32>,
    service_max_stream_duration: Option<u32>,
    cluster_send_timeout: Option<u32>,
    cluster_read_timeout: Option<u32>,
    cluster_max_stream_duration: Option<u32>,
    service_retry: RetrySettings,
    cluster_retry: RetrySettings,
) -> String {
    let mut tera = Tera::default();
    register_filters(&mut tera);
    tera.add_raw_template("template", HTTP_TEMPLATE)
        .expect("HTTP template should parse");

    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert("gateway_http_routes_per_namespace", &json!({"app-ns": [{}]}));
    context.insert("sanitized_name", &"router-name");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("labels_group", &json!({ "common": {} }));
    context.insert(
        "advanced_settings",
        &base_advanced_settings(
            service_request_timeout,
            service_idle_timeout,
            service_max_stream_duration,
            &service_retry,
        ),
    );
    context.insert("cluster_envoy_gateway_api_http_request_timeout_seconds", &cluster_send_timeout);
    context.insert(
        "cluster_envoy_gateway_api_http_connection_idle_timeout_seconds",
        &cluster_read_timeout,
    );
    context.insert(
        "cluster_envoy_gateway_api_http_max_stream_duration_seconds",
        &cluster_max_stream_duration,
    );
    context.insert("cluster_envoy_gateway_api_retry_num_retries", &cluster_retry.num_retries);
    context.insert("cluster_envoy_gateway_api_retry_retry_on", &cluster_retry.retry_on);
    context.insert(
        "cluster_envoy_gateway_api_retry_http_status_codes",
        &cluster_retry.http_status_codes,
    );
    context.insert(
        "cluster_envoy_gateway_api_retry_per_try_timeout_seconds",
        &cluster_retry.per_try_timeout_seconds,
    );
    let resolved_retry = resolve_retry_settings(&service_retry, &cluster_retry);
    context.insert("resolved_gateway_api_retry_num_retries", &resolved_retry.num_retries);
    context.insert(
        "resolved_gateway_api_retry_retry_on_triggers",
        &resolved_retry.retry_on_triggers,
    );
    context.insert(
        "resolved_gateway_api_retry_http_status_codes",
        &resolved_retry.http_status_codes,
    );
    context.insert(
        "resolved_gateway_api_retry_per_try_timeout_seconds",
        &resolved_retry.per_try_timeout_seconds,
    );

    tera.render("template", &context).expect("HTTP template should render")
}

fn render_grpc_policy(
    service_request_timeout: Option<u32>,
    service_idle_timeout: Option<u32>,
    service_max_stream_duration: Option<u32>,
    cluster_send_timeout: Option<u32>,
    cluster_read_timeout: Option<u32>,
    cluster_max_stream_duration: Option<u32>,
) -> String {
    render_grpc_policy_with_retry(
        service_request_timeout,
        service_idle_timeout,
        service_max_stream_duration,
        cluster_send_timeout,
        cluster_read_timeout,
        cluster_max_stream_duration,
        no_retry_settings(),
        no_retry_settings(),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_grpc_policy_with_retry(
    service_request_timeout: Option<u32>,
    service_idle_timeout: Option<u32>,
    service_max_stream_duration: Option<u32>,
    cluster_send_timeout: Option<u32>,
    cluster_read_timeout: Option<u32>,
    cluster_max_stream_duration: Option<u32>,
    service_retry: RetrySettings,
    cluster_retry: RetrySettings,
) -> String {
    let mut tera = Tera::default();
    register_filters(&mut tera);
    tera.add_raw_template("template", GRPC_TEMPLATE)
        .expect("gRPC template should parse");

    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert(
        "grpc_hosts_per_namespace_gateway",
        &json!({"app-ns": [{"domain_name": "example.com"}]}),
    );
    context.insert("sanitized_name", &"router-name");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("labels_group", &json!({ "common": {} }));
    context.insert(
        "advanced_settings",
        &base_advanced_settings(
            service_request_timeout,
            service_idle_timeout,
            service_max_stream_duration,
            &service_retry,
        ),
    );
    context.insert("cluster_envoy_gateway_api_http_request_timeout_seconds", &cluster_send_timeout);
    context.insert(
        "cluster_envoy_gateway_api_http_connection_idle_timeout_seconds",
        &cluster_read_timeout,
    );
    context.insert(
        "cluster_envoy_gateway_api_http_max_stream_duration_seconds",
        &cluster_max_stream_duration,
    );
    context.insert("cluster_envoy_gateway_api_retry_num_retries", &cluster_retry.num_retries);
    context.insert("cluster_envoy_gateway_api_retry_retry_on", &cluster_retry.retry_on);
    context.insert(
        "cluster_envoy_gateway_api_retry_http_status_codes",
        &cluster_retry.http_status_codes,
    );
    context.insert(
        "cluster_envoy_gateway_api_retry_per_try_timeout_seconds",
        &cluster_retry.per_try_timeout_seconds,
    );
    let resolved_retry = resolve_retry_settings(&service_retry, &cluster_retry);
    context.insert("resolved_gateway_api_retry_num_retries", &resolved_retry.num_retries);
    context.insert(
        "resolved_gateway_api_retry_retry_on_triggers",
        &resolved_retry.retry_on_triggers,
    );
    context.insert(
        "resolved_gateway_api_retry_http_status_codes",
        &resolved_retry.http_status_codes,
    );
    context.insert(
        "resolved_gateway_api_retry_per_try_timeout_seconds",
        &resolved_retry.per_try_timeout_seconds,
    );

    tera.render("template", &context).expect("gRPC template should render")
}

fn render_http_route() -> String {
    let mut tera = Tera::default();
    register_filters(&mut tera);
    tera.add_raw_template("template", HTTP_ROUTE_TEMPLATE)
        .expect("HTTP route template should parse");

    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert(
        "gateway_http_routes_per_namespace",
        &json!({
            "app-ns": [{
                "hostnames": ["example.com"],
                "rules": [{
                    "service_name": "demo",
                    "service_port": 80,
                    "weight": 1,
                    "path_type": "PathPrefix",
                    "path": "/",
                    "path_rewrite": null
                }]
            }]
        }),
    );
    context.insert("sanitized_name", &"router-name");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("has_wildcard_domain", &false);
    context.insert("certificate_alternative_names", &json!([]));
    context.insert("k8s_deploy_listenerset", &false);
    context.insert(
        "labels_group",
        &json!({ "common": { "labels-group-key": "labels-group-value" } }),
    );
    context.insert(
        "annotations_group",
        &json!({ "gateway_api_routes": { "annotations-group-key": "annotations-group-value" } }),
    );
    context.insert(
        "advanced_settings",
        &json!({
            "network_gateway_api_force_ssl_redirect": false,
            "network_gateway_api_add_headers": {},
            "network_gateway_api_proxy_set_headers": {}
        }),
    );

    tera.render("template", &context)
        .expect("HTTP route template should render")
}

fn render_grpc_route() -> String {
    let mut tera = Tera::default();
    register_filters(&mut tera);
    tera.add_raw_template("template", GRPC_ROUTE_TEMPLATE)
        .expect("gRPC route template should parse");

    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert(
        "grpc_hosts_per_namespace_gateway",
        &json!({
            "app-ns": [{
                "domain_name": "example.com",
                "service_name": "demo",
                "service_port": 50051,
                "weight": 1
            }]
        }),
    );
    context.insert("sanitized_name", &"router-name");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("has_wildcard_domain", &false);
    context.insert("certificate_alternative_names", &json!([]));
    context.insert("k8s_deploy_listenerset", &false);
    context.insert(
        "labels_group",
        &json!({ "common": { "labels-group-key": "labels-group-value" } }),
    );
    context.insert(
        "annotations_group",
        &json!({ "gateway_api_routes": { "annotations-group-key": "annotations-group-value" } }),
    );
    context.insert(
        "advanced_settings",
        &json!({
            "network_gateway_api_force_ssl_redirect": false,
            "network_gateway_api_add_headers": {},
            "network_gateway_api_proxy_set_headers": {}
        }),
    );

    tera.render("template", &context)
        .expect("gRPC route template should render")
}

#[test]
fn http_policy_uses_cluster_defaults_when_service_timeout_is_missing() {
    let rendered = render_http_policy(None, None, None, Some(42), Some(120), Some(600));
    assert!(rendered.contains("requestTimeout: 42s"));
    assert!(rendered.contains("connectionIdleTimeout: 120s"));
    assert!(rendered.contains("maxStreamDuration: 600s"));
}

#[test]
fn http_policy_prioritizes_service_timeout_over_cluster_default() {
    let rendered = render_http_policy(Some(90), None, None, Some(42), Some(120), Some(600));
    assert!(rendered.contains("requestTimeout: 90s"));
    assert!(!rendered.contains("requestTimeout: 42s"));
    assert!(rendered.contains("connectionIdleTimeout: 120s"));
    assert!(rendered.contains("maxStreamDuration: 600s"));
}

#[test]
fn http_policy_prioritizes_service_idle_timeout_over_cluster_default() {
    let rendered = render_http_policy(None, Some(121), None, Some(42), Some(120), Some(600));
    assert!(rendered.contains("requestTimeout: 42s"));
    assert!(rendered.contains("connectionIdleTimeout: 121s"));
    assert!(!rendered.contains("connectionIdleTimeout: 120s"));
    assert!(rendered.contains("maxStreamDuration: 600s"));
}

#[test]
fn http_policy_omits_timeout_when_no_value_is_provided() {
    let rendered = render_http_policy(None, None, None, None, None, None);
    assert!(!rendered.contains("requestTimeout:"));
    assert!(!rendered.contains("connectionIdleTimeout:"));
    assert!(!rendered.contains("maxStreamDuration:"));
}

#[test]
fn grpc_policy_uses_cluster_defaults_and_service_override() {
    let rendered = render_grpc_policy(Some(75), Some(121), Some(300), Some(42), Some(120), Some(600));
    assert!(rendered.contains("requestTimeout: 75s"));
    assert!(rendered.contains("connectionIdleTimeout: 121s"));
    assert!(rendered.contains("maxStreamDuration: 300s"));
}

#[test]
fn http_policy_prioritizes_service_max_stream_duration_over_cluster_default() {
    let rendered = render_http_policy(None, None, Some(300), Some(42), Some(120), Some(600));
    assert!(rendered.contains("maxStreamDuration: 300s"));
    assert!(!rendered.contains("maxStreamDuration: 600s"));
}

#[test]
fn with_no_retry_settings_retry_block_is_not_rendered() {
    let rendered = render_http_policy(None, None, None, None, None, None);
    assert!(!rendered.contains("\n  retry:"));
}

#[test]
fn service_num_retries_renders_retry_num_retries() {
    let rendered = render_http_policy_with_retry(
        None,
        None,
        None,
        None,
        None,
        None,
        RetrySettings {
            num_retries: Some(2),
            ..no_retry_settings()
        },
        no_retry_settings(),
    );
    assert!(rendered.contains("numRetries: 2"));
}

#[test]
fn service_retry_on_renders_retry_on_triggers() {
    let rendered = render_http_policy_with_retry(
        None,
        None,
        None,
        None,
        None,
        None,
        RetrySettings {
            num_retries: Some(2),
            retry_on: "connect-failure, reset,refused-stream".to_string(),
            ..no_retry_settings()
        },
        no_retry_settings(),
    );
    assert!(rendered.contains("triggers:"));
    assert!(rendered.contains("- connect-failure"));
    assert!(rendered.contains("- reset"));
    assert!(rendered.contains("- refused-stream"));
}

#[test]
fn service_http_status_codes_renders_retry_on_http_status_codes() {
    let rendered = render_http_policy_with_retry(
        None,
        None,
        None,
        None,
        None,
        None,
        RetrySettings {
            num_retries: Some(2),
            http_status_codes: "503,504".to_string(),
            ..no_retry_settings()
        },
        no_retry_settings(),
    );
    assert!(rendered.contains("httpStatusCodes:"));
    assert!(rendered.contains("- 503"));
    assert!(rendered.contains("- 504"));
}

#[test]
fn service_per_try_timeout_seconds_renders_per_retry_timeout() {
    let rendered = render_http_policy_with_retry(
        None,
        None,
        None,
        None,
        None,
        None,
        RetrySettings {
            num_retries: Some(2),
            per_try_timeout_seconds: Some(2),
            ..no_retry_settings()
        },
        no_retry_settings(),
    );
    assert!(rendered.contains("perRetry:"));
    assert!(rendered.contains("timeout: 2s"));
}

#[test]
fn service_settings_override_cluster_settings() {
    let rendered = render_http_policy_with_retry(
        None,
        None,
        None,
        None,
        None,
        None,
        RetrySettings {
            num_retries: Some(2),
            retry_on: "connect-failure".to_string(),
            http_status_codes: "503".to_string(),
            per_try_timeout_seconds: Some(2),
        },
        RetrySettings {
            num_retries: Some(4),
            retry_on: "reset".to_string(),
            http_status_codes: "504".to_string(),
            per_try_timeout_seconds: Some(5),
        },
    );
    assert!(rendered.contains("numRetries: 2"));
    assert!(!rendered.contains("numRetries: 4"));
    assert!(rendered.contains("- connect-failure"));
    assert!(!rendered.contains("- reset"));
    assert!(rendered.contains("- 503"));
    assert!(!rendered.contains("- 504"));
    assert!(rendered.contains("timeout: 2s"));
    assert!(!rendered.contains("timeout: 5s"));
}

#[test]
fn cluster_settings_are_used_when_service_settings_are_absent() {
    let rendered = render_http_policy_with_retry(
        None,
        None,
        None,
        None,
        None,
        None,
        no_retry_settings(),
        RetrySettings {
            num_retries: Some(2),
            retry_on: "connect-failure, reset, refused-stream".to_string(),
            http_status_codes: "503".to_string(),
            per_try_timeout_seconds: Some(2),
        },
    );
    assert!(rendered.contains("numRetries: 2"));
    assert!(rendered.contains("- connect-failure"));
    assert!(rendered.contains("- reset"));
    assert!(rendered.contains("- refused-stream"));
    assert!(rendered.contains("- 503"));
    assert!(rendered.contains("timeout: 2s"));
}

#[test]
fn num_retries_zero_renders_retry_num_retries_zero() {
    let rendered = render_http_policy_with_retry(
        None,
        None,
        None,
        None,
        None,
        None,
        RetrySettings {
            num_retries: Some(0),
            ..no_retry_settings()
        },
        no_retry_settings(),
    );
    assert!(rendered.contains("numRetries: 0"));
}

#[test]
fn http_and_grpc_templates_render_retry_consistently() {
    let service_retry = RetrySettings {
        num_retries: Some(2),
        retry_on: "retriable-status-codes".to_string(),
        http_status_codes: "503".to_string(),
        per_try_timeout_seconds: Some(2),
    };
    let rendered_http =
        render_http_policy_with_retry(None, None, None, None, None, None, service_retry.clone(), no_retry_settings());
    let rendered_grpc =
        render_grpc_policy_with_retry(None, None, None, None, None, None, service_retry, no_retry_settings());

    for snippet in [
        "numRetries: 2",
        "retryOn:",
        "- retriable-status-codes",
        "httpStatusCodes:",
        "- 503",
        "perRetry:",
        "timeout: 2s",
    ] {
        assert!(rendered_http.contains(snippet));
        assert!(rendered_grpc.contains(snippet));
    }
}

#[test]
fn http_route_includes_group_annotations_and_labels() {
    let rendered = render_http_route();
    assert!(rendered.contains(r#""annotations-group-key": "annotations-group-value""#));
    assert!(rendered.contains(r#""labels-group-key": "labels-group-value""#));
}

#[test]
fn grpc_route_includes_group_annotations_and_labels() {
    let rendered = render_grpc_route();
    assert!(rendered.contains(r#""annotations-group-key": "annotations-group-value""#));
    assert!(rendered.contains(r#""labels-group-key": "labels-group-value""#));
}
