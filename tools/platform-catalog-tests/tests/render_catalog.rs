use platform_catalog_tests::{
    REGISTRY, contains_string_fragment, parse_yaml_file, repository_path, run, yaml_path, yaml_string,
};
use serde_json::json;
use serde_yaml::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CATALOG_VERSION: &str = "2026-07-20.1";
const SELF_MANAGED_TEMPLATE: &str = "platform-catalog/templates/qovery-self-managed-v0/template.yaml";
const QCORE_CLOUD_VENDORS: [&str; 11] = [
    "AWS", "SCW", "GCP", "DO", "AZURE", "OVH", "CIVO", "HETZNER", "ORACLE", "IBM", "UNKNOWN",
];

fn layer_components(template: &Value, layer_key: &str) -> Vec<String> {
    yaml_path(template, &["platformTemplateRelease", "layers"])
        .and_then(Value::as_sequence)
        .expect("template must declare layers")
        .iter()
        .find(|layer| yaml_string(layer, &["key"]) == Some(layer_key))
        .and_then(|layer| yaml_path(layer, &["components"]))
        .and_then(Value::as_sequence)
        .unwrap_or_else(|| panic!("layer {layer_key} must declare components"))
        .iter()
        .map(|component| {
            yaml_string(component, &["key"])
                .unwrap_or_else(|| panic!("component in layer {layer_key} must have a key"))
                .to_owned()
        })
        .collect()
}

fn write_template_output(path: &Path, version: &str) {
    let output = json!([
        {
            "key": "qovery-cluster-v0",
            "version": version,
            "ref": format!("{REGISTRY}/platform-templates/qovery-cluster-v0:{version}"),
            "digest": DIGEST,
        },
        {
            "key": "qovery-demo-v0",
            "version": version,
            "ref": format!("{REGISTRY}/platform-templates/qovery-demo-v0:{version}"),
            "digest": DIGEST,
        },
        {
            "key": "qovery-self-managed-v0",
            "version": version,
            "ref": format!("{REGISTRY}/platform-templates/qovery-self-managed-v0:{version}"),
            "digest": DIGEST,
        },
    ]);
    fs::write(path, serde_json::to_vec(&output).expect("publication output must serialize"))
        .expect("publication output must be writable");
}

fn render_catalog(input: &Path, destination: &Path) -> std::process::Output {
    run(Command::new(repository_path("scripts/publish-platform-catalog.sh")).args([
        "render-catalog",
        input.to_str().expect("temporary path must be UTF-8"),
        destination.to_str().expect("temporary path must be UTF-8"),
        CATALOG_VERSION,
        REGISTRY,
    ]))
}

#[test]
fn template_layers_keep_the_expected_component_order() {
    let template = parse_yaml_file(repository_path("platform-catalog/templates/qovery-cluster-v0/template.yaml"));

    assert_eq!(
        layer_components(&template, "qovery-stack"),
        ["cluster-agent", "shell-agent", "qovery-priority-class"]
    );
    assert_eq!(layer_components(&template, "log-infra"), ["loki", "alloy"]);
    assert_eq!(
        layer_components(&template, "gateway-api"),
        [
            "envoy-gateway-crd",
            "envoy-gateway",
            "qovery-gateway-class",
            "qovery-cluster-gateway",
        ]
    );
    assert_eq!(
        layer_components(&template, "dns-certificates"),
        [
            "cert-manager",
            "qovery-cert-manager-webhook",
            "external-dns-secret",
            "external-dns",
            "cert-manager-configs",
        ]
    );

    let layers = yaml_path(&template, &["platformTemplateRelease", "layers"])
        .and_then(Value::as_sequence)
        .expect("template must declare layers");
    assert!(
        !layers
            .iter()
            .any(|layer| { matches!(yaml_string(layer, &["key"]), Some("cluster-foundation" | "log-collector")) })
    );
}

#[test]
fn demo_template_contains_only_the_legacy_demo_components() {
    let template = parse_yaml_file(repository_path("platform-catalog/templates/qovery-demo-v0/template.yaml"));

    assert_eq!(
        layer_components(&template, "qovery-stack"),
        ["cluster-agent", "shell-agent", "qovery-priority-class"]
    );
    assert_eq!(
        layer_components(&template, "gateway-api"),
        [
            "envoy-gateway-crd",
            "envoy-gateway",
            "qovery-gateway-class",
            "qovery-cluster-gateway",
        ]
    );
    assert_eq!(
        layer_components(&template, "dns-certificates"),
        [
            "cert-manager",
            "qovery-cert-manager-webhook",
            "external-dns-secret",
            "external-dns",
            "cert-manager-configs",
        ]
    );

    let layers = yaml_path(&template, &["platformTemplateRelease", "layers"])
        .and_then(Value::as_sequence)
        .expect("demo template must declare layers");
    assert_eq!(layers.len(), 3);
    for layer in layers {
        let key = yaml_string(layer, &["key"]).expect("demo layer must declare its key");
        assert_eq!(
            yaml_path(layer, &["mandatory"]).and_then(Value::as_bool),
            Some(key != "gateway-api"),
            "only the demo gateway layer is optional"
        );
        assert_eq!(
            yaml_path(layer, &["enabledByDefault"]).and_then(Value::as_bool),
            Some(true),
            "demo layer {key} must remain enabled by default"
        );
    }
    assert!(!contains_component(&template, "loki"));
    assert!(!contains_component(&template, "alloy"));
    assert!(!contains_component(&template, "qovery-engine"));
    assert!(!contains_component(&template, "ingress-nginx"));
}

#[test]
fn gateway_layer_is_optional_and_keeps_its_execution_dependencies() {
    for template_path in [
        "platform-catalog/templates/qovery-cluster-v0/template.yaml",
        "platform-catalog/templates/qovery-demo-v0/template.yaml",
    ] {
        let template = parse_yaml_file(repository_path(template_path));
        let gateway_layer = yaml_path(&template, &["platformTemplateRelease", "layers"])
            .and_then(Value::as_sequence)
            .and_then(|layers| {
                layers
                    .iter()
                    .find(|layer| yaml_string(layer, &["key"]) == Some("gateway-api"))
            })
            .expect("template must declare the gateway API layer");
        assert_eq!(
            yaml_path(gateway_layer, &["mandatory"]).and_then(Value::as_bool),
            Some(false),
            "{template_path} must allow disabling the gateway API layer"
        );
        assert_eq!(
            yaml_path(gateway_layer, &["enabledByDefault"]).and_then(Value::as_bool),
            Some(true),
            "{template_path} must keep the gateway API layer enabled by default"
        );
        assert_cluster_gateway_prerequisites(&template, template_path);
    }
}

fn assert_cluster_gateway_prerequisites(template: &Value, template_path: &str) {
    let cluster_gateway = component(template, "qovery-cluster-gateway");
    let dependencies = yaml_path(cluster_gateway, &["dependsOn"])
        .and_then(Value::as_sequence)
        .expect("cluster gateway must declare its prerequisites");
    let prerequisite_keys = dependencies
        .iter()
        .filter(|dependency| yaml_string(dependency, &["kind"]) == Some("requires"))
        .filter_map(|dependency| yaml_string(dependency, &["component"]))
        .collect::<Vec<_>>();
    assert_eq!(
        prerequisite_keys,
        ["envoy-gateway-crd", "envoy-gateway", "qovery-gateway-class"],
        "{template_path} must create the Gateway only after its API, controller, and class"
    );

    let input = yaml_path(cluster_gateway, &["runtimeInputs"])
        .and_then(Value::as_sequence)
        .and_then(|inputs| {
            inputs
                .iter()
                .find(|input| yaml_string(input, &["name"]) == Some("dns.managedDomain"))
        })
        .expect("cluster gateway must declare its managed DNS input");
    assert_eq!(yaml_string(input, &["source", "key"]), Some("dns.managedDomain"));
}

fn template_layers(template: &Value) -> &Vec<Value> {
    yaml_path(template, &["platformTemplateRelease", "layers"])
        .and_then(Value::as_sequence)
        .expect("template must declare layers")
}

fn layer_applies_to(layer: &Value, mode: &str, provider: &str) -> bool {
    let declared = |field: &str| {
        yaml_path(layer, &["applicability", field])
            .and_then(Value::as_sequence)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
    };
    let modes = declared("modes").expect("layer applicability must declare its modes");
    modes.contains(&mode) && declared("providers").is_none_or(|providers| providers.contains(&provider))
}

#[test]
fn catalog_keeps_the_cluster_default_and_declares_the_self_managed_template() {
    let catalog = parse_yaml_file(repository_path("platform-catalog/catalog.yaml"));
    assert_eq!(yaml_string(&catalog, &["defaultTemplate", "key"]), Some("qovery-cluster-v0"));
    assert_eq!(yaml_string(&catalog, &["defaultTemplate", "version"]), Some("0.1.0"));
    let declarations = yaml_path(&catalog, &["templates"])
        .and_then(Value::as_sequence)
        .expect("catalog must declare its templates")
        .iter()
        .filter(|declaration| yaml_string(declaration, &["key"]) == Some("qovery-self-managed-v0"))
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 1, "the self-managed template must be declared once");
    assert_eq!(yaml_string(declarations[0], &["version"]), Some("0.1.0"));
    assert_eq!(yaml_string(declarations[0], &["path"]), Some(SELF_MANAGED_TEMPLATE));

    let template = parse_yaml_file(repository_path(SELF_MANAGED_TEMPLATE));
    assert_eq!(
        yaml_string(&template, &["platformTemplateRelease", "key"]),
        Some("qovery-self-managed-v0")
    );
    assert_eq!(yaml_string(&template, &["platformTemplateRelease", "version"]), Some("0.1.0"));
    assert_eq!(
        yaml_string(&template, &["platformTemplateRelease", "status"]),
        Some("PUBLISHED"),
        "q-core only binds PUBLISHED releases"
    );
}

#[test]
fn self_managed_template_makes_every_layer_mandatory_without_karpenter_or_infrastructure() {
    let template = parse_yaml_file(repository_path(SELF_MANAGED_TEMPLATE));
    let layers = template_layers(&template);
    assert_eq!(
        layers
            .iter()
            .map(|layer| yaml_string(layer, &["key"]).expect("layer must declare its key"))
            .collect::<Vec<_>>(),
        ["qovery-stack", "log-infra", "gateway-api", "dns-certificates"]
    );
    for layer in layers {
        let key = yaml_string(layer, &["key"]).expect("layer must declare its key");
        assert_eq!(
            yaml_path(layer, &["mandatory"]).and_then(Value::as_bool),
            Some(true),
            "layer {key} must be mandatory"
        );
        assert_eq!(
            yaml_path(layer, &["enabledByDefault"]).and_then(Value::as_bool),
            Some(true),
            "layer {key} must stay enabled by default"
        );
    }
    assert_eq!(
        layer_components(&template, "qovery-stack"),
        ["cluster-agent", "shell-agent", "qovery-priority-class"]
    );
    assert_eq!(layer_components(&template, "log-infra"), ["loki", "alloy"]);
    assert_eq!(
        layer_components(&template, "gateway-api"),
        [
            "envoy-gateway-crd",
            "envoy-gateway",
            "qovery-gateway-class",
            "qovery-cluster-gateway",
        ]
    );
    assert_eq!(
        layer_components(&template, "dns-certificates"),
        [
            "cert-manager",
            "qovery-cert-manager-webhook",
            "external-dns-secret",
            "external-dns",
            "cert-manager-configs",
        ]
    );
    assert!(
        !contains_string_fragment(&template, "karpenter"),
        "the self-managed template must not declare or reference Karpenter components"
    );
    assert_cluster_gateway_prerequisites(&template, SELF_MANAGED_TEMPLATE);
}

#[test]
fn self_managed_mandatory_layers_resolve_their_requirements_in_every_cluster_context() {
    let template = parse_yaml_file(repository_path(SELF_MANAGED_TEMPLATE));
    let layers = template_layers(&template);
    assert!(
        layers
            .iter()
            .all(|layer| layer_applies_to(layer, "CUSTOMER_MANAGED", "AWS")),
        "every mandatory layer must deploy on a customer-managed AWS cluster"
    );
    for mode in ["QOVERY_MANAGED", "CUSTOMER_MANAGED"] {
        for provider in QCORE_CLOUD_VENDORS {
            let components = layers
                .iter()
                .filter(|layer| layer_applies_to(layer, mode, provider))
                .flat_map(|layer| {
                    yaml_path(layer, &["components"])
                        .and_then(Value::as_sequence)
                        .expect("layer must declare components")
                })
                .collect::<Vec<_>>();
            let deployed = components
                .iter()
                .map(|component| yaml_string(component, &["key"]).expect("component must declare its key"))
                .collect::<BTreeSet<_>>();
            for component in &components {
                let key = yaml_string(component, &["key"]).expect("component must declare its key");
                for dependency in yaml_path(component, &["dependsOn"])
                    .and_then(Value::as_sequence)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    if yaml_string(dependency, &["kind"]).unwrap_or("requires") != "requires" {
                        continue;
                    }
                    let required = yaml_string(dependency, &["component"]).expect("dependency must name a component");
                    assert!(
                        deployed.contains(required),
                        "{key} requires {required}, which is not deployed on a {mode}/{provider} cluster"
                    );
                }
            }
        }
    }
}

fn contains_component(template: &Value, component_key: &str) -> bool {
    yaml_path(template, &["platformTemplateRelease", "layers"])
        .and_then(Value::as_sequence)
        .is_some_and(|layers| {
            layers.iter().any(|layer| {
                yaml_path(layer, &["components"])
                    .and_then(Value::as_sequence)
                    .is_some_and(|components| {
                        components
                            .iter()
                            .any(|component| yaml_string(component, &["key"]) == Some(component_key))
                    })
            })
        })
}

fn component<'a>(template: &'a Value, component_key: &str) -> &'a Value {
    yaml_path(template, &["platformTemplateRelease", "layers"])
        .and_then(Value::as_sequence)
        .expect("template must declare layers")
        .iter()
        .filter_map(|layer| yaml_path(layer, &["components"]).and_then(Value::as_sequence))
        .flat_map(|components| components.iter())
        .find(|component| yaml_string(component, &["key"]) == Some(component_key))
        .unwrap_or_else(|| panic!("template must declare component {component_key}"))
}

#[test]
fn platform_image_tags_are_required_qcore_values_without_template_fallbacks() {
    for template_path in [
        "platform-catalog/templates/qovery-cluster-v0/template.yaml",
        "platform-catalog/templates/qovery-demo-v0/template.yaml",
        SELF_MANAGED_TEMPLATE,
    ] {
        let template = parse_yaml_file(repository_path(template_path));
        let bootstrap_component = yaml_path(&template, &["platformTemplateRelease", "bootstrap", "component"])
            .expect("template must declare its bootstrap component");
        for (component_key, component, input_name, source_key) in [
            (
                "cluster-agent",
                component(&template, "cluster-agent"),
                "image.tag",
                "clusterAgent.imageTag",
            ),
            (
                "shell-agent",
                component(&template, "shell-agent"),
                "image.tag",
                "shellAgent.imageTag",
            ),
            ("qovery-operator", bootstrap_component, "operator.imageTag", "operator.imageTag"),
        ] {
            assert!(
                yaml_path(&template, &["platformTemplateRelease", "runtimeSourceValues", source_key]).is_none(),
                "{template_path} must not provide the environment-owned {source_key} value"
            );

            let image_tag_input = yaml_path(component, &["runtimeInputs"])
                .and_then(Value::as_sequence)
                .expect("component must declare runtime inputs")
                .iter()
                .find(|input| yaml_string(input, &["name"]) == Some(input_name))
                .unwrap_or_else(|| panic!("{component_key} must declare {input_name}"));

            assert_eq!(yaml_string(image_tag_input, &["source", "kind"]), Some("qcoreValue"));
            assert_eq!(yaml_string(image_tag_input, &["source", "key"]), Some(source_key));
            assert_eq!(yaml_path(image_tag_input, &["required"]).and_then(Value::as_bool), Some(true));
        }
    }
}

#[test]
fn complete_template_publication_renders_a_digest_pinned_catalog() {
    let temporary_directory = TempDir::new().expect("temporary directory must be created");
    let input = temporary_directory.path().join("templates.json");
    let destination = temporary_directory.path().join("catalog.yaml");
    write_template_output(&input, "0.1.0");

    let output = render_catalog(&input, &destination);
    assert!(
        output.status.success(),
        "render-catalog failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let catalog = parse_yaml_file(&destination);
    assert_eq!(yaml_string(&catalog, &["apiVersion"]), Some("platform.qovery.com/v1alpha1"));
    assert_eq!(yaml_string(&catalog, &["kind"]), Some("PlatformTemplateCatalog"));
    assert_eq!(yaml_string(&catalog, &["version"]), Some(CATALOG_VERSION));
    assert_eq!(yaml_string(&catalog, &["defaultRelease", "key"]), Some("qovery-cluster-v0"));
    assert_eq!(yaml_string(&catalog, &["defaultRelease", "version"]), Some("0.1.0"));

    let releases = yaml_path(&catalog, &["releases"])
        .and_then(Value::as_sequence)
        .expect("catalog must contain releases");
    assert_eq!(releases.len(), 3);
    let release = releases
        .iter()
        .find(|release| yaml_string(release, &["key"]) == Some("qovery-cluster-v0"))
        .expect("catalog must contain its default release");
    assert_eq!(
        yaml_string(release, &["repository"]),
        Some("public.ecr.aws/r3m4q3r9/platform-templates/qovery-cluster-v0")
    );
    assert_eq!(yaml_string(release, &["digest"]), Some(DIGEST));

    let demo_release = releases
        .iter()
        .find(|release| yaml_string(release, &["key"]) == Some("qovery-demo-v0"))
        .expect("catalog must contain its demo release");
    assert_eq!(
        yaml_string(demo_release, &["repository"]),
        Some("public.ecr.aws/r3m4q3r9/platform-templates/qovery-demo-v0")
    );
    assert_eq!(yaml_string(demo_release, &["digest"]), Some(DIGEST));

    let self_managed_release = releases
        .iter()
        .find(|release| yaml_string(release, &["key"]) == Some("qovery-self-managed-v0"))
        .expect("catalog must contain the self-managed release");
    assert_eq!(yaml_string(self_managed_release, &["version"]), Some("0.1.0"));
    assert_eq!(
        yaml_string(self_managed_release, &["repository"]),
        Some("public.ecr.aws/r3m4q3r9/platform-templates/qovery-self-managed-v0")
    );
    assert_eq!(yaml_string(self_managed_release, &["digest"]), Some(DIGEST));
}

#[test]
fn partial_template_publication_is_rejected_without_writing_a_catalog() {
    let temporary_directory = TempDir::new().expect("temporary directory must be created");
    let input = temporary_directory.path().join("templates.json");
    let destination = temporary_directory.path().join("catalog.yaml");
    fs::write(&input, "[]\n").expect("publication output must be writable");

    let output = render_catalog(&input, &destination);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a valid complete template publication output"));
    assert!(!destination.exists());
}

#[test]
fn mismatched_template_coordinate_is_rejected_without_writing_a_catalog() {
    let temporary_directory = TempDir::new().expect("temporary directory must be created");
    let input = temporary_directory.path().join("templates.json");
    let destination = temporary_directory.path().join("catalog.yaml");
    write_template_output(&input, "0.2.0");

    let output = render_catalog(&input, &destination);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("catalog snapshot requires every declared template release")
    );
    assert!(!destination.exists());
}
