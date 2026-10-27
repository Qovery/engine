//! Render the EnvoyProxy resources that replace the controller's default configuration.
use platform_catalog_tests::{
    contains_key, document_by_kind_and_name, document_kind, helm_template, parse_yaml_documents, parse_yaml_file,
    repository_path, yaml_path, yaml_string,
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

#[test]
fn catalog_pins_the_controller_default_security_image() {
    // setup:
    let values_path = repository_path("platform-catalog/components/envoy-gateway/config/static-values/base.yaml");
    let values = parse_yaml_file(&values_path);
    assert_eq!(
        yaml_string(&values, &["global", "images", "envoyProxy", "image"]),
        Some(SECURITY_IMAGE),
        "the catalog must pin the image independently of upstream chart defaults"
    );

    // execute:
    let documents = parse_yaml_documents(&helm_template(
        "envoy-image-test",
        "lib-engine/lib/common/bootstrap/charts/envoy-gateway",
        "qovery",
        &[values_path],
        &[],
    ));

    // validate:
    let config_map = document_by_kind_and_name(&documents, "ConfigMap", "envoy-gateway-config")
        .expect("the controller configuration must be rendered");
    let config = yaml_string(config_map, &["data", "envoy-gateway.yaml"])
        .expect("the ConfigMap must contain the controller configuration");
    let config: Value = serde_yaml::from_str(config).expect("the controller configuration must be valid YAML");
    assert_eq!(
        yaml_string(
            &config,
            &[
                "envoyProxy",
                "provider",
                "kubernetes",
                "envoyDeployment",
                "container",
                "image"
            ],
        ),
        Some(SECURITY_IMAGE),
    );
}

#[test]
fn catalog_pins_every_qovery_proxy_security_image_without_merging() {
    // setup:
    let mut proxies = Vec::new();
    for component in ["qovery-gateway-class", "qovery-cluster-gateway"] {
        let values_path = repository_path(format!(
            "platform-catalog/components/{component}/config/static-values/base.yaml"
        ));
        let values = parse_yaml_file(&values_path);
        assert_eq!(
            yaml_string(&values, &["envoyImage"]),
            Some(SECURITY_IMAGE),
            "{component} must pin the image independently of chart defaults"
        );

        // execute:
        let chart = format!("lib-engine/lib/common/bootstrap/charts/{component}");
        proxies.extend(
            parse_yaml_documents(&helm_template("envoy-image-test", &chart, "qovery", &[values_path], &[]))
                .into_iter()
                .filter(|document| document_kind(document) == Some("EnvoyProxy")),
        );
    }

    // validate:
    assert_proxy_images(&proxies, SECURITY_IMAGE);
}
