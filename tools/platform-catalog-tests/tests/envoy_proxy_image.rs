//! Render the EnvoyProxy resources that replace the controller's default configuration.
use platform_catalog_tests::{
    contains_key, document_by_kind_and_name, document_kind, helm_template, parse_yaml_documents, yaml_path, yaml_string,
};
use serde_yaml::Value;

const SECURITY_IMAGE: &str = "docker.io/envoyproxy/envoy:distroless-v1.39.5";
const PROXY_NAMES: [&str; 3] = [
    "qovery-public-envoy-proxy-config",
    "qovery-private-envoy-proxy-config",
    "qovery-cluster-public-gateway-envoy-proxy",
];

fn render_proxies(class_arguments: &[&str], cluster_arguments: &[&str]) -> Vec<Value> {
    [
        ("lib-engine/lib/common/bootstrap/charts/qovery-gateway-class", class_arguments),
        (
            "lib-engine/lib/common/bootstrap/charts/qovery-cluster-gateway",
            cluster_arguments,
        ),
    ]
    .into_iter()
    .flat_map(|(chart, arguments)| {
        parse_yaml_documents(&helm_template("envoy-image-test", chart, "qovery", &[], arguments))
    })
    .filter(|document| document_kind(document) == Some("EnvoyProxy"))
    .collect()
}

fn assert_proxy_images(proxies: &[Value], expected_image: &str) {
    assert_eq!(proxies.len(), PROXY_NAMES.len());
    for name in PROXY_NAMES {
        let proxy = document_by_kind_and_name(proxies, "EnvoyProxy", name)
            .unwrap_or_else(|| panic!("missing EnvoyProxy/{name}"));
        assert_eq!(
            yaml_string(
                proxy,
                &[
                    "spec",
                    "provider",
                    "kubernetes",
                    "envoyDeployment",
                    "container",
                    "image",
                ],
            ),
            Some(expected_image),
            "EnvoyProxy/{name} must set its own image because it replaces the controller defaults"
        );
        assert!(
            !contains_key(proxy, "mergeType"),
            "EnvoyProxy/{name} must retain replacement semantics"
        );
    }
}

#[test]
fn every_qovery_proxy_explicitly_uses_the_security_image_without_merging() {
    // setup:
    let expected_image = SECURITY_IMAGE;

    // execute:
    let proxies = render_proxies(&[], &[]);

    // validate:
    assert_proxy_images(&proxies, expected_image);
}

#[test]
fn security_image_is_rendered_even_without_optional_pod_settings() {
    // setup:
    let class_arguments = [
        "--set-json",
        "gatewayClass.qoveryPublic.affinity={},gatewayClass.qoveryPublic.topologySpreadConstraints=[],gatewayClass.qoveryPublic.tolerations=[],gatewayClass.qoveryPrivate.affinity={},gatewayClass.qoveryPrivate.topologySpreadConstraints=[],gatewayClass.qoveryPrivate.tolerations=[]",
    ];
    let cluster_arguments = [
        "--set-json",
        "envoyProxy.qoveryPublic.provider.kubernetes.envoyDeployment.pod.affinity={},envoyProxy.qoveryPublic.provider.kubernetes.envoyDeployment.pod.topologySpreadConstraints=[],envoyProxy.qoveryPublic.provider.kubernetes.envoyDeployment.pod.tolerations=[]",
    ];

    // execute:
    let proxies = render_proxies(&class_arguments, &cluster_arguments);

    // validate:
    assert_proxy_images(&proxies, SECURITY_IMAGE);
    for proxy in &proxies {
        for field in ["affinity", "topologySpreadConstraints", "tolerations"] {
            assert!(
                yaml_path(proxy, &["spec", "provider", "kubernetes", "envoyDeployment", "pod", field]).is_none(),
                "the empty pod settings fixture must not render {field}"
            );
        }
    }
}

#[test]
fn chart_image_setting_reaches_every_proxy_without_merging() {
    // setup:
    let expected_image = "registry.example.com/envoy:custom";
    let arguments = ["--set-string", "envoyImage=registry.example.com/envoy:custom"];

    // execute:
    let proxies = render_proxies(&arguments, &arguments);

    // validate:
    assert_proxy_images(&proxies, expected_image);
}
