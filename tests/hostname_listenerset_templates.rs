//! Covers the templates of the `PER_HOSTNAME_LISTENER_SET` routing mode: one ListenerSet per hostname,
//! and the route parents that attach to it.

#[path = "helpers/native_helm.rs"]
mod native_helm;

use native_helm::{render_per_hostname_router_template, render_router_template};
use serde_json::{Value, json};
use serde_yaml::Value as YamlValue;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tera::Context;

const HTTP_ROUTE_TEMPLATE: &str = include_str!("../lib/common/charts/q-ingress-tls/templates/gateway-http-route.yaml");
const GRPC_ROUTE_TEMPLATE: &str = include_str!("../lib/common/charts/q-ingress-tls/templates/gateway-grpc-route.yaml");
const PER_HOSTNAME_HTTP_ROUTE_TEMPLATE: &str =
    include_str!("../lib/common/charts/q-ingress-tls-per-hostname/templates/gateway-http-route.yaml");
const PER_HOSTNAME_GRPC_ROUTE_TEMPLATE: &str =
    include_str!("../lib/common/charts/q-ingress-tls-per-hostname/templates/gateway-grpc-route.yaml");
const LEGACY_LISTENER_SET_TEMPLATE: &str =
    include_str!("../lib/common/charts/q-ingress-tls/templates/listenerset.yaml");
const PER_HOSTNAME_CHART_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/lib/common/charts/q-ingress-tls-per-hostname");
const HOSTNAME_CHART_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/lib/common/charts/q-hostname-listenerset");

const LISTENER_SET_NAME: &str = "listenerset-0123456789abcdef";
const TLS_SECRET_NAME: &str = "listenerset-0123456789abcdef-tls";

fn manifests(rendered: &str) -> Vec<YamlValue> {
    rendered
        .split("\n---")
        .filter_map(|document| {
            let document = document.trim().trim_start_matches("---").trim();
            let has_content = document
                .lines()
                .any(|line| !line.trim().is_empty() && !line.trim().starts_with('#'));
            has_content.then(|| serde_yaml::from_str(document).expect("rendered manifest must parse as YAML"))
        })
        .collect()
}

fn render_hostname_chart(namespaces: &[&str]) -> Vec<YamlValue> {
    let values = tempfile::NamedTempFile::new().expect("values file");
    fs::write(
        values.path(),
        json!({
            "name": LISTENER_SET_NAME,
            "hostname": "api.example.com",
            "tls_secret_name": TLS_SECRET_NAME,
            "namespaces": namespaces,
            "router_long_id": "router-long-id",
            "router_name": "api-router",
            "environment_long_id": "environment-id",
            "project_long_id": "project-id",
        })
        .to_string(),
    )
    .expect("write values");
    let output = Command::new("helm")
        .args(["template", "test", HOSTNAME_CHART_PATH, "--values"])
        .arg(values.path())
        .output()
        .expect("helm must be installed to test chart rendering");
    assert!(
        output.status.success(),
        "helm template failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    manifests(&String::from_utf8(output.stdout).expect("Helm output must be UTF-8"))
}

fn route_context(listener_set_names: Value) -> Context {
    let mut context = Context::new();
    context.insert("k8s_deploy_api_gateway", &true);
    context.insert("k8s_deploy_listenerset", &true);
    context.insert("hostname_listener_set_names", &listener_set_names);
    context.insert(
        "gateway_http_routes_per_namespace",
        &json!({
            "app-ns": [{
                "hostnames": ["api.example.com", "p8000.api.example.com", "zabc-gtw.rustrocks.me"],
                "rules": [{
                    "service_name": "api-svc",
                    "service_port": 8080,
                    "weight": 1,
                    "path_type": "PathPrefix",
                    "path": "/api",
                    "path_rewrite": null
                }]
            }]
        }),
    );
    context.insert(
        "grpc_hosts_per_namespace_gateway",
        &json!({
            "app-ns": [
                {
                    "domain_name": "  Grpc.Example.com.\t",
                    "service_name": "grpc-svc",
                    "service_port": 50051,
                    "weight": 1
                },
                {
                    "domain_name": " zabc-gtw.rustrocks.me ",
                    "service_name": "grpc-svc",
                    "service_port": 50051,
                    "weight": 1
                }
            ]
        }),
    );
    context.insert("sanitized_name", &"api-router");
    context.insert("long_id", &"service-id");
    context.insert("associated_service_long_id", &"associated-service-id");
    context.insert("associated_service_type", &"application");
    context.insert("environment_long_id", &"environment-id");
    context.insert("project_long_id", &"project-id");
    context.insert("has_wildcard_domain", &false);
    context.insert(
        "certificate_alternative_names",
        &json!([{ "domain": "api.example.com" }, { "domain": "p8000.api.example.com" }]),
    );
    context.insert(
        "http_hosts_per_namespace_gateway",
        &json!({ "app-ns": [{ "domain_name": "api.example.com" }] }),
    );
    context.insert("labels_group", &json!({ "common": {} }));
    context.insert("annotations_group", &json!({ "gateway_api_routes": {} }));
    context.insert(
        "advanced_settings",
        &json!({
            "network_gateway_api_force_ssl_redirect": false,
            "network_gateway_api_add_headers": {},
            "network_gateway_api_proxy_set_headers": {}
        }),
    );
    context
}

fn hostname_listener_set_names() -> Value {
    json!({
        "app-ns": {
            "api.example.com": "listenerset-aaaaaaaaaaaaaaaa",
            "p8000.api.example.com": "listenerset-bbbbbbbbbbbbbbbb",
            "grpc.example.com": "listenerset-cccccccccccccccc"
        }
    })
}

fn parent_refs(route: &YamlValue) -> Vec<&YamlValue> {
    route["spec"]["parentRefs"]
        .as_sequence()
        .expect("route must have parentRefs")
        .iter()
        .collect()
}

fn listener_set_parent_names(route: &YamlValue) -> Vec<String> {
    parent_refs(route)
        .into_iter()
        .filter(|parent| parent["kind"].as_str() == Some("ListenerSet"))
        .map(|parent| {
            assert_eq!(parent["group"].as_str(), Some("gateway.networking.k8s.io"));
            assert_eq!(
                parent["namespace"].as_str(),
                Some("app-ns"),
                "a ListenerSet only accepts routes of its own namespace"
            );
            parent["name"].as_str().expect("ListenerSet parent name").to_string()
        })
        .collect()
}

fn gateway_section_names(route: &YamlValue) -> Vec<String> {
    parent_refs(route)
        .into_iter()
        .filter(|parent| parent["kind"].is_null())
        .map(|parent| {
            assert_eq!(parent["name"].as_str(), Some("qovery-cluster-public-gateway"));
            parent["sectionName"].as_str().expect("Gateway sectionName").to_string()
        })
        .collect()
}

#[test]
fn hostname_chart_renders_one_listener_set_with_a_single_hostname() {
    let rendered = render_hostname_chart(&["app-ns"]);
    let listener_set = rendered
        .iter()
        .find(|manifest| manifest["kind"].as_str() == Some("ListenerSet"))
        .expect("a ListenerSet must be rendered");

    assert_eq!(listener_set["metadata"]["name"].as_str(), Some(LISTENER_SET_NAME));
    assert_eq!(listener_set["metadata"]["namespace"].as_str(), Some("app-ns"));
    assert_eq!(
        listener_set["spec"]["parentRef"]["name"].as_str(),
        Some("qovery-cluster-public-gateway")
    );
    assert_eq!(listener_set["spec"]["parentRef"]["namespace"].as_str(), Some("qovery"));

    let listeners = listener_set["spec"]["listeners"].as_sequence().expect("listeners");
    assert_eq!(listeners.len(), 1, "a ListenerSet carries exactly one hostname");
    assert_eq!(listeners[0]["hostname"].as_str(), Some("api.example.com"));
    assert_eq!(listeners[0]["protocol"].as_str(), Some("HTTPS"));
    assert_eq!(listeners[0]["port"].as_u64(), Some(443));
    assert_eq!(listeners[0]["allowedRoutes"]["namespaces"]["from"].as_str(), Some("Same"));
    assert_eq!(listeners[0]["tls"]["mode"].as_str(), Some("Terminate"));
    assert_eq!(listeners[0]["tls"]["certificateRefs"][0]["kind"].as_str(), Some("Secret"));
    assert_eq!(
        listeners[0]["tls"]["certificateRefs"][0]["name"].as_str(),
        Some(TLS_SECRET_NAME)
    );
    assert_eq!(listeners[0]["tls"]["certificateRefs"][0]["namespace"].as_str(), Some("app-ns"));
}

#[test]
fn hostname_chart_certificate_is_the_only_owner_of_the_tls_secret() {
    let rendered = render_hostname_chart(&["app-ns"]);
    let listener_set = rendered
        .iter()
        .find(|manifest| manifest["kind"].as_str() == Some("ListenerSet"))
        .expect("a ListenerSet must be rendered");
    let certificate = rendered
        .iter()
        .find(|manifest| manifest["kind"].as_str() == Some("Certificate"))
        .expect("a Certificate must be rendered");

    // The Gateway shim of cert-manager would create a second Certificate for the same Secret.
    assert!(
        listener_set["metadata"].get("annotations").is_none(),
        "the ListenerSet must not ask cert-manager to issue a certificate"
    );
    assert_eq!(certificate["metadata"]["name"].as_str(), Some(TLS_SECRET_NAME));
    assert_eq!(certificate["metadata"]["namespace"].as_str(), Some("app-ns"));
    assert_eq!(certificate["spec"]["secretName"].as_str(), Some(TLS_SECRET_NAME));
    assert_eq!(certificate["spec"]["dnsNames"].as_sequence().map(Vec::len), Some(1));
    assert_eq!(certificate["spec"]["dnsNames"][0].as_str(), Some("api.example.com"));
    assert_eq!(certificate["spec"]["issuerRef"]["kind"].as_str(), Some("ClusterIssuer"));
    assert_eq!(certificate["spec"]["issuerRef"]["name"].as_str(), Some("letsencrypt-qovery"));
}

#[test]
fn hostname_chart_labels_record_the_router_that_owns_the_release() {
    let rendered = render_hostname_chart(&["app-ns"]);

    for manifest in &rendered {
        let labels = &manifest["metadata"]["labels"];
        // The engine reads the owner of an installed release from these labels in the persisted Helm manifest,
        // every object of the release must carry it, see `release_owner_from_manifest`.
        assert_eq!(labels["qovery.com/service-id"].as_str(), Some("router-long-id"));
        assert_eq!(labels["qovery.com/service-type"].as_str(), Some("router"));
        assert_eq!(labels["qovery.com/environment-id"].as_str(), Some("environment-id"));
        assert_eq!(labels["qovery.com/project-id"].as_str(), Some("project-id"));
        assert_eq!(labels["qovery.com/listenerset-scope"].as_str(), Some("hostname"));
    }
}

#[test]
fn hostname_chart_renders_a_listener_set_and_a_certificate_per_route_namespace() {
    let rendered = render_hostname_chart(&["app-ns", "other-ns"]);

    let namespaces_of = |kind: &str| -> Vec<&str> {
        rendered
            .iter()
            .filter(|manifest| manifest["kind"].as_str() == Some(kind))
            .map(|manifest| manifest["metadata"]["namespace"].as_str().expect("namespace"))
            .collect()
    };
    assert_eq!(namespaces_of("ListenerSet"), vec!["app-ns", "other-ns"]);
    assert_eq!(namespaces_of("Certificate"), vec!["app-ns", "other-ns"]);
}

#[test]
fn http_route_attaches_to_the_listener_set_of_each_custom_hostname_and_keeps_its_backend() {
    let context = route_context(hostname_listener_set_names());
    let rendered = render_per_hostname_router_template(PER_HOSTNAME_HTTP_ROUTE_TEMPLATE, &context)
        .expect("HTTP route should render");
    let routes = manifests(&rendered);
    let route = routes
        .iter()
        .find(|route| route["metadata"]["name"].as_str() == Some("api-router"))
        .expect("the HTTPS route must be rendered");

    assert_eq!(
        listener_set_parent_names(route),
        vec![
            "listenerset-aaaaaaaaaaaaaaaa".to_string(),
            "listenerset-bbbbbbbbbbbbbbbb".to_string()
        ],
        "only the custom hostnames get a ListenerSet parent, not the platform hostname:\n{rendered}"
    );
    assert!(
        listener_set_parent_names(route)
            .iter()
            .all(|name| !name.ends_with("-listeners")),
        "the router-wide legacy ListenerSet must not be referenced"
    );
    assert_eq!(
        gateway_section_names(route),
        vec!["https".to_string(), "http".to_string()],
        "the platform hostname keeps the Gateway listeners"
    );
    assert_eq!(route["spec"]["hostnames"].as_sequence().map(Vec::len), Some(3));
    assert_eq!(route["spec"]["rules"][0]["backendRefs"][0]["name"].as_str(), Some("api-svc"));
    assert_eq!(route["spec"]["rules"][0]["backendRefs"][0]["port"].as_u64(), Some(8080));
    assert_eq!(route["spec"]["rules"][0]["matches"][0]["path"]["value"].as_str(), Some("/api"));
}

#[test]
fn http_route_attaches_to_the_listener_set_name_it_is_given() {
    let given_names = json!({ "app-ns": { "api.example.com": "listenerset-dddddddddddddddd" } });
    let rendered = render_per_hostname_router_template(PER_HOSTNAME_HTTP_ROUTE_TEMPLATE, &route_context(given_names))
        .expect("HTTP route should render");
    let routes = manifests(&rendered);
    let route = routes
        .iter()
        .find(|route| route["metadata"]["name"].as_str() == Some("api-router"))
        .expect("the HTTPS route must be rendered");

    assert_eq!(
        listener_set_parent_names(route),
        vec!["listenerset-dddddddddddddddd".to_string()]
    );
}

#[test]
fn routers_sharing_a_hostname_attach_to_one_listener_set_and_keep_their_own_backends() {
    let names = json!({ "app-ns": { "api.example.com": LISTENER_SET_NAME } });
    let mut routes = Vec::new();
    for (router, backend, path) in [
        ("rails-router", "rails-svc", "/"),
        ("cable-router", "cable-svc", "/cable"),
    ] {
        let mut context = route_context(names.clone());
        context.insert("sanitized_name", &router);
        context.insert(
            "gateway_http_routes_per_namespace",
            &json!({
                "app-ns": [{
                    "hostnames": ["api.example.com"],
                    "rules": [{
                        "service_name": backend,
                        "service_port": 8080,
                        "weight": 1,
                        "path_type": "PathPrefix",
                        "path": path,
                        "path_rewrite": null
                    }]
                }]
            }),
        );
        let rendered = render_per_hostname_router_template(PER_HOSTNAME_HTTP_ROUTE_TEMPLATE, &context)
            .expect("shared hostname route should render");
        routes.extend(manifests(&rendered));
    }

    assert_eq!(routes.len(), 2);
    for (route, (router, backend, path)) in routes.iter().zip([
        ("rails-router", "rails-svc", "/"),
        ("cable-router", "cable-svc", "/cable"),
    ]) {
        assert_eq!(route["metadata"]["name"].as_str(), Some(router));
        assert_eq!(listener_set_parent_names(route), vec![LISTENER_SET_NAME]);
        assert_eq!(route["spec"]["hostnames"][0].as_str(), Some("api.example.com"));
        assert_eq!(route["spec"]["rules"][0]["backendRefs"][0]["name"].as_str(), Some(backend));
        assert_eq!(route["spec"]["rules"][0]["matches"][0]["path"]["value"].as_str(), Some(path));
    }
}

#[test]
fn grpc_route_attaches_to_the_listener_set_of_its_custom_hostname() {
    let context = route_context(hostname_listener_set_names());
    let rendered = render_per_hostname_router_template(PER_HOSTNAME_GRPC_ROUTE_TEMPLATE, &context)
        .expect("gRPC route should render");
    let routes = manifests(&rendered);
    let custom_route = routes
        .iter()
        .find(|route| route["spec"]["hostnames"][0].as_str() == Some("grpc.example.com"))
        .expect("route of the custom gRPC hostname, normalized");
    let platform_route = routes
        .iter()
        .find(|route| route["spec"]["hostnames"][0].as_str() == Some("zabc-gtw.rustrocks.me"))
        .expect("route of the platform gRPC hostname");

    assert_eq!(
        listener_set_parent_names(custom_route),
        vec!["listenerset-cccccccccccccccc".to_string()],
        "hostnames are matched trimmed, case-insensitively and without their trailing dot"
    );
    assert!(listener_set_parent_names(platform_route).is_empty());
    assert_eq!(
        custom_route["spec"]["rules"][0]["backendRefs"][0]["name"].as_str(),
        Some("grpc-svc")
    );
    assert_eq!(
        gateway_section_names(platform_route),
        vec!["https".to_string(), "http".to_string()]
    );
}

/// Gateway API `Hostname`: lowercase RFC 1123 labels separated by dots, no trailing dot.
fn is_gateway_api_hostname(hostname: &str) -> bool {
    !hostname.is_empty()
        && hostname.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn route_hostnames(route: &YamlValue) -> Vec<String> {
    route["spec"]["hostnames"]
        .as_sequence()
        .expect("route must have hostnames")
        .iter()
        .map(|hostname| hostname.as_str().expect("hostname").to_string())
        .collect()
}

#[test]
fn emitted_http_route_hostnames_are_normalized_in_per_hostname_mode() {
    let mut context = route_context(hostname_listener_set_names());
    context.insert(
        "gateway_http_routes_per_namespace",
        &json!({
            "app-ns": [{
                "hostnames": [" API.Example.com. ", "p8000.api.example.com", "api.example.com", "\tZABC-gtw.rustrocks.me.\n"],
                "rules": [{
                    "service_name": "api-svc",
                    "service_port": 8080,
                    "weight": 1,
                    "path_type": "PathPrefix",
                    "path": "/api",
                    "path_rewrite": null
                }]
            }]
        }),
    );
    context.insert(
        "advanced_settings",
        &json!({
            "network_gateway_api_force_ssl_redirect": true,
            "network_gateway_api_add_headers": {},
            "network_gateway_api_proxy_set_headers": {}
        }),
    );
    let rendered = render_per_hostname_router_template(PER_HOSTNAME_HTTP_ROUTE_TEMPLATE, &context)
        .expect("HTTP route should render");
    let routes = manifests(&rendered);
    let main_route = routes
        .iter()
        .find(|route| route["metadata"]["name"].as_str() == Some("api-router"))
        .expect("the HTTPS route must be rendered");
    let redirect_route = routes
        .iter()
        .find(|route| route["metadata"]["name"].as_str() == Some("api-router-ssl-redir"))
        .expect("the redirect route must be rendered");

    for route in [main_route, redirect_route] {
        let hostnames = route_hostnames(route);
        assert_eq!(
            hostnames,
            vec![
                "api.example.com".to_string(),
                "p8000.api.example.com".to_string(),
                "zabc-gtw.rustrocks.me".to_string()
            ],
            "hostnames must be lowercase, without trailing dot and deduplicated:\n{rendered}"
        );
        assert!(hostnames.iter().all(|hostname| is_gateway_api_hostname(hostname)));
    }
    assert_eq!(
        listener_set_parent_names(main_route),
        vec![
            "listenerset-aaaaaaaaaaaaaaaa".to_string(),
            "listenerset-bbbbbbbbbbbbbbbb".to_string()
        ],
        "the padded, mixed-case hostname still finds its ListenerSet"
    );
}

#[test]
fn emitted_grpc_route_hostnames_are_normalized_in_per_hostname_mode() {
    let rendered = render_per_hostname_router_template(
        PER_HOSTNAME_GRPC_ROUTE_TEMPLATE,
        &route_context(hostname_listener_set_names()),
    )
    .expect("gRPC route should render");

    let mut all_hostnames = Vec::new();
    for route in manifests(&rendered) {
        let hostnames = route_hostnames(&route);
        assert!(
            hostnames.iter().all(|hostname| is_gateway_api_hostname(hostname)),
            "{hostnames:?} must follow the Gateway API hostname syntax:\n{rendered}"
        );
        all_hostnames.extend(hostnames);
    }
    assert_eq!(
        all_hostnames,
        // The template always emits the `-gtw.new-gateway-api.` variant line, which repeats the hostname when it
        // has no `-gtw.` part. This is the existing behavior and it is not changed here.
        vec![
            "grpc.example.com".to_string(),
            "grpc.example.com".to_string(),
            "zabc-gtw.rustrocks.me".to_string(),
            "zabc-gtw.new-gateway-api.rustrocks.me".to_string()
        ],
        "surrounding whitespace, case and the trailing dot are all removed:\n{rendered}"
    );
}

#[test]
fn legacy_mode_keeps_the_route_hostnames_as_rendered_before() {
    let rendered =
        render_router_template(GRPC_ROUTE_TEMPLATE, &route_context(json!({}))).expect("gRPC route should render");
    let routes = manifests(&rendered);

    assert!(
        routes
            .iter()
            .any(|route| route["spec"]["hostnames"][0].as_str() == Some("  Grpc.Example.com.\t")),
        "legacy hostnames are not rewritten:\n{rendered}"
    );
}

#[test]
fn legacy_chart_keeps_the_router_wide_listener_set_parent_and_ignores_hostname_listener_sets() {
    let without_names = render_router_template(HTTP_ROUTE_TEMPLATE, &route_context(json!({})))
        .expect("legacy HTTP route should render");
    let with_names = render_router_template(HTTP_ROUTE_TEMPLATE, &route_context(hostname_listener_set_names()))
        .expect("legacy HTTP route should render");

    assert_eq!(
        without_names, with_names,
        "the legacy chart must not read the per hostname ListenerSet names"
    );
    let routes = manifests(&without_names);
    let route = routes
        .iter()
        .find(|route| route["metadata"]["name"].as_str() == Some("api-router"))
        .expect("the HTTPS route must be rendered");
    assert_eq!(listener_set_parent_names(route), vec!["api-router-listeners".to_string()]);
}

#[test]
fn per_hostname_chart_renders_no_router_wide_listener_set_or_client_policy() {
    let mut context = route_context(hostname_listener_set_names());
    context.insert("k8s_deploy_client_traffic_policy_listenerset", &true);

    let legacy = render_router_template(LEGACY_LISTENER_SET_TEMPLATE, &context).expect("legacy ListenerSet");
    assert_eq!(
        manifests(&legacy)[0]["metadata"]["name"].as_str(),
        Some("api-router-listeners"),
        "the legacy chart still renders the router-wide ListenerSet"
    );

    let templates = PathBuf::from(PER_HOSTNAME_CHART_PATH).join("templates");
    for file in ["listenerset.yaml", "gateway-client-traffic-policy.yaml"] {
        assert!(
            !templates.join(file).exists(),
            "{file} is legacy only and must not exist in the per hostname chart"
        );
    }
    let rendered = render_per_hostname_router_template(PER_HOSTNAME_HTTP_ROUTE_TEMPLATE, &context)
        .expect("per hostname HTTP route should render");
    assert!(
        manifests(&rendered)
            .iter()
            .all(|manifest| !matches!(manifest["kind"].as_str(), Some("ListenerSet" | "ClientTrafficPolicy"))),
        "the per hostname chart must not render the router-wide ListenerSet or ClientTrafficPolicy:\n{rendered}"
    );
}
