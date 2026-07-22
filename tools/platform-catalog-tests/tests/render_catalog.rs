use platform_catalog_tests::{REGISTRY, parse_yaml_file, repository_path, run, yaml_path, yaml_string};
use serde_json::json;
use serde_yaml::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CATALOG_VERSION: &str = "2026-07-20.1";

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
    let output = json!([{
        "key": "qovery-cluster-v0",
        "version": version,
        "ref": format!("{REGISTRY}/platform-templates/qovery-cluster-v0:{version}"),
        "digest": DIGEST,
    }]);
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

    let release = yaml_path(&catalog, &["releases"])
        .and_then(Value::as_sequence)
        .and_then(|releases| releases.first())
        .expect("catalog must contain its default release");
    assert_eq!(
        yaml_string(release, &["repository"]),
        Some("public.ecr.aws/r3m4q3r9/platform-templates/qovery-cluster-v0")
    );
    assert_eq!(yaml_string(release, &["digest"]), Some(DIGEST));
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
