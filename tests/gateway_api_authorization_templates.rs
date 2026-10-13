//! IP authorization must preserve deny precedence for both HTTP and gRPC routes.

#[path = "helpers/native_helm.rs"]
mod native_helm;

use native_helm::render_router_template;
use serde_json::{Value, json};
use tera::Context;

const TEMPLATES: [&str; 2] = [
    include_str!("../lib/common/charts/q-ingress-tls/templates/gateway-http-route-envoy-security-policy.yaml"),
    include_str!("../lib/common/charts/q-ingress-tls/templates/gateway-grpc-route-envoy-security-policy.yaml"),
];

fn render_policy(template: &str, whitelist: &str, denylist: &str, gateway_enabled: bool) -> String {
    let context = Context::from_value(json!({
        "k8s_deploy_api_gateway": gateway_enabled,
        "gateway_http_routes_per_namespace": {"app-ns": [{}]},
        "grpc_hosts_per_namespace_gateway": {"app-ns": [{}]},
        "sanitized_name": "router-name",
        "long_id": "service-id",
        "associated_service_long_id": "associated-service-id",
        "associated_service_type": "application",
        "environment_long_id": "environment-id",
        "project_long_id": "project-id",
        "labels_group": {"common": {}},
        "advanced_settings": {
            "network_gateway_api_whitelist_source_range": whitelist,
            "network_gateway_api_denylist_source_range": denylist
        }
    }))
    .expect("valid router context");

    render_router_template(template, &context).expect("SecurityPolicy should render")
}

fn authorization(template: &str, whitelist: &str, denylist: &str) -> Value {
    let rendered = render_policy(template, whitelist, denylist, true);
    let policy: Value = serde_yaml::from_str(&rendered).expect("SecurityPolicy should be valid YAML");
    assert_eq!(policy["kind"], "SecurityPolicy");
    policy["spec"]["authorization"].clone()
}

#[test]
fn denylist_precedes_overlapping_whitelist() {
    for template in TEMPLATES {
        // Includes subnet overlap, identical CIDRs, IPv4, IPv6, and CSV whitespace.
        let policy = authorization(
            template,
            "10.0.0.0/8, 172.16.0.0/12, 2001:db8::/32",
            "10.10.10.0/24, 172.16.0.0/12, 2001:db8:1::/48",
        );

        // Envoy applies the first matching rule, so overlapping clients must hit Deny first.
        assert_eq!(
            policy,
            json!({
                "defaultAction": "Deny",
                "rules": [
                    {
                        "action": "Deny",
                        "principal": {"clientCIDRs": ["10.10.10.0/24", "172.16.0.0/12", "2001:db8:1::/48"]}
                    },
                    {
                        "action": "Allow",
                        "principal": {"clientCIDRs": ["10.0.0.0/8", "172.16.0.0/12", "2001:db8::/32"]}
                    }
                ]
            })
        );
    }
}

#[test]
fn whitelist_only_defaults_to_deny() {
    for template in TEMPLATES {
        assert_eq!(
            authorization(template, "10.0.0.0/8", ""),
            json!({
                "defaultAction": "Deny",
                "rules": [{"action": "Allow", "principal": {"clientCIDRs": ["10.0.0.0/8"]}}]
            })
        );
    }
}

#[test]
fn denylist_with_unrestricted_whitelist_defaults_to_allow() {
    for template in TEMPLATES {
        for whitelist in ["", "0.0.0.0/0"] {
            assert_eq!(
                authorization(template, whitelist, "10.10.10.0/24"),
                json!({
                    "defaultAction": "Allow",
                    "rules": [{"action": "Deny", "principal": {"clientCIDRs": ["10.10.10.0/24"]}}]
                })
            );
        }
    }
}

#[test]
fn unrestricted_whitelist_without_denylist_omits_authorization() {
    for template in TEMPLATES {
        for whitelist in ["", "0.0.0.0/0"] {
            assert_eq!(authorization(template, whitelist, ""), Value::Null);
        }
    }
}

#[test]
fn disabled_gateway_omits_security_policy() {
    for template in TEMPLATES {
        assert!(
            render_policy(template, "10.0.0.0/8", "10.10.10.0/24", false)
                .trim()
                .is_empty()
        );
    }
}
