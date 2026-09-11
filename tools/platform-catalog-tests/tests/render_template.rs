use platform_catalog_tests::{
    REGISTRY, contains_string, mapping_string, mappings_for_key, parse_yaml_file, repository_path, run, yaml_path,
    yaml_string,
};
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use serde_yaml::Value as YamlValue;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const CHART_DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Deserialize)]
struct Catalog {
    components: Vec<CatalogComponent>,
    charts: Vec<CatalogChart>,
    templates: Vec<CatalogTemplate>,
}

#[derive(Deserialize)]
struct CatalogComponent {
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct CatalogChart {
    name: String,
    path: String,
}

#[derive(Deserialize)]
struct CatalogTemplate {
    path: String,
}

#[derive(Deserialize)]
struct ChartMetadata {
    name: String,
    version: String,
}

struct RenderFixture {
    _temporary_directory: TempDir,
    destination: PathBuf,
    config_output: PathBuf,
    chart_output: PathBuf,
    config_entries: Vec<JsonValue>,
    chart_entries: Vec<JsonValue>,
}

impl RenderFixture {
    fn new() -> Self {
        let temporary_directory = TempDir::new().expect("temporary directory must be created");
        let catalog = read_catalog();
        let config_entries = catalog
            .components
            .iter()
            .enumerate()
            .map(|(index, component)| config_entry(&component.name, &component.version, index + 1))
            .collect();
        let chart_entries = catalog
            .charts
            .iter()
            .map(|chart| {
                let metadata = read_chart_metadata(&chart.path);
                assert_eq!(
                    metadata.name, chart.name,
                    "catalog chart name must match the Chart.yaml name at {}",
                    chart.path
                );
                chart_entry(&chart.name, &metadata.version)
            })
            .collect();

        Self {
            destination: temporary_directory.path().join("template.yaml"),
            config_output: temporary_directory.path().join("platform-config-publish.json"),
            chart_output: temporary_directory.path().join("frozen-charts-publish.json"),
            _temporary_directory: temporary_directory,
            config_entries,
            chart_entries,
        }
    }

    fn write_outputs(&self) {
        write_json(&self.config_output, &self.config_entries);
        write_json(&self.chart_output, &self.chart_entries);
    }

    fn render(&self, version: &str) -> Output {
        self.render_source(
            "platform-catalog/templates/qovery-cluster-v0/template.yaml",
            "qovery-cluster-v0",
            version,
        )
    }

    fn render_source(&self, source: &str, key: &str, version: &str) -> Output {
        run(Command::new(repository_path("scripts/publish-platform-catalog.sh")).args([
            "render",
            repository_path(source).to_str().expect("repository path must be UTF-8"),
            self.config_output.to_str().expect("temporary path must be UTF-8"),
            self.chart_output.to_str().expect("temporary path must be UTF-8"),
            self.destination.to_str().expect("temporary path must be UTF-8"),
            key,
            version,
            REGISTRY,
        ]))
    }
}

fn read_catalog() -> Catalog {
    let path = repository_path("platform-catalog/catalog.yaml");
    let source = fs::read_to_string(&path).expect("platform catalog must be readable");
    serde_yaml::from_str(&source).expect("platform catalog must match the test schema")
}

fn read_chart_metadata(chart_path: &str) -> ChartMetadata {
    let path = repository_path(chart_path).join("Chart.yaml");
    let source = fs::read_to_string(&path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_yaml::from_str(&source)
        .unwrap_or_else(|error| panic!("{} must contain valid chart metadata: {error}", path.display()))
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec(value).expect("JSON value must serialize"))
        .expect("JSON output must be writable");
}

fn config_entry(component: &str, version: &str, digest_seed: usize) -> JsonValue {
    json!({
        "component": component,
        "version": version,
        "ref": format!("{REGISTRY}/platform-config/{component}:{version}"),
        "digest": format!("sha256:{digest_seed:064x}"),
    })
}

fn chart_entry(chart: &str, version: &str) -> JsonValue {
    json!({
        "chart": chart,
        "version": version,
        "ref": format!("{REGISTRY}/charts/{chart}:{version}"),
        "digest": CHART_DIGEST,
    })
}

fn assert_config_references_preserved(source_path: &str, rendered: &YamlValue) {
    fn coordinates(template: &YamlValue) -> Vec<(String, String)> {
        let mut references = Vec::new();
        mappings_for_key(template, "configRef", &mut references);
        let mut coordinates = references
            .into_iter()
            .map(|reference| {
                (
                    mapping_string(reference, "chart").expect("config chart").to_owned(),
                    mapping_string(reference, "version").expect("config version").to_owned(),
                )
            })
            .collect::<Vec<_>>();
        coordinates.sort();
        coordinates
    }
    let source = parse_yaml_file(repository_path(source_path));
    assert_eq!(
        coordinates(rendered),
        coordinates(&source),
        "rendering must preserve every source configRef, including multiplicity"
    );
}

#[test]
fn every_config_reference_is_pinned_from_verified_publication_outputs() {
    let fixture = RenderFixture::new();
    fixture.write_outputs();

    let output = fixture.render("0.1.0");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered_source = fs::read_to_string(&fixture.destination).expect("rendered template must exist");
    assert!(!rendered_source.contains("__PUBLISHED_CONFIG_DIGEST__"));
    let rendered = parse_yaml_file(&fixture.destination);
    assert_config_references_preserved("platform-catalog/templates/qovery-cluster-v0/template.yaml", &rendered);
    let mut config_references = Vec::new();
    mappings_for_key(&rendered, "configRef", &mut config_references);

    assert!(!config_references.is_empty());
    for reference in config_references {
        assert!(
            fixture.config_entries.iter().any(|entry| {
                mapping_string(reference, "chart") == entry["component"].as_str()
                    && mapping_string(reference, "version") == entry["version"].as_str()
                    && mapping_string(reference, "digest") == entry["digest"].as_str()
            }),
            "rendered config reference has no matching verified publication: {reference:?}"
        );
    }
    assert!(contains_string(&rendered, "oci://public.ecr.aws/r3m4q3r9/charts/"));
}

#[test]
fn demo_template_renders_from_the_verified_publication_outputs() {
    let fixture = RenderFixture::new();
    fixture.write_outputs();

    let output = fixture.render_source(
        "platform-catalog/templates/qovery-demo-v0/template.yaml",
        "qovery-demo-v0",
        "0.1.0",
    );

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered_source = fs::read_to_string(&fixture.destination).expect("rendered template must exist");
    assert!(!rendered_source.contains("__PUBLISHED_CONFIG_DIGEST__"));
    assert_config_references_preserved(
        "platform-catalog/templates/qovery-demo-v0/template.yaml",
        &parse_yaml_file(&fixture.destination),
    );
}

#[test]
fn published_components_and_charts_are_referenced_by_the_template_set() {
    let catalog = read_catalog();
    let expected_configs = catalog
        .components
        .iter()
        .map(|component| (component.name.clone(), component.version.clone()))
        .collect::<BTreeSet<_>>();
    let expected_charts = catalog
        .charts
        .iter()
        .map(|chart| {
            let metadata = read_chart_metadata(&chart.path);
            (chart.name.clone(), metadata.version)
        })
        .collect::<BTreeSet<_>>();

    let mut referenced_configs = BTreeSet::new();
    let mut referenced_charts = BTreeSet::new();
    for catalog_template in &catalog.templates {
        let template = parse_yaml_file(repository_path(&catalog_template.path));
        let mut config_references = Vec::new();
        mappings_for_key(&template, "configRef", &mut config_references);
        referenced_configs.extend(config_references.into_iter().map(|reference| {
            (
                mapping_string(reference, "chart")
                    .expect("config reference must declare a chart")
                    .to_owned(),
                mapping_string(reference, "version")
                    .expect("config reference must declare a version")
                    .to_owned(),
            )
        }));

        let mut chart_references = Vec::new();
        mappings_for_key(&template, "chart", &mut chart_references);
        referenced_charts.extend(chart_references.into_iter().map(|reference| {
            (
                mapping_string(reference, "name")
                    .expect("chart reference must declare a name")
                    .to_owned(),
                mapping_string(reference, "version")
                    .expect("chart reference must declare a version")
                    .to_owned(),
            )
        }));
    }

    assert_eq!(referenced_configs, expected_configs);
    assert_eq!(referenced_charts, expected_charts);
}

#[test]
fn catalog_components_use_the_expected_release_identities() {
    for catalog_template in read_catalog().templates {
        let template: JsonValue =
            serde_yaml::from_str(&fs::read_to_string(repository_path(&catalog_template.path)).unwrap()).unwrap();
        let release = &template["platformTemplateRelease"];
        let components = release["layers"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|layer| layer["components"].as_array().unwrap())
            .chain(release["bootstrap"].get("component"));
        for component in components {
            let key = component["key"].as_str().unwrap();
            let expected_chart = match key {
                "karpenter-crd" | "karpenter" => Some(key),
                "karpenter-configuration" => Some("karpenter-custom-resources"),
                _ => None,
            };
            if let Some(chart) = expected_chart {
                assert_eq!(component["kind"], "HELM");
                assert_eq!(component["release"]["name"], key);
                assert_eq!(component["release"]["namespace"], "kube-system");
                assert_eq!(component["chart"]["name"], chart);
                assert_eq!(component["chart"]["repository"], "oci://public.ecr.aws/r3m4q3r9/charts/");
                assert_eq!(component["configRef"]["chart"], key);
            } else {
                assert_eq!(component["release"]["namespace"], "qovery", "{key}");
            }
        }
    }
}

#[test]
fn demo_worker_configuration_comes_from_the_operator_evaluator() {
    let template = parse_yaml_file(repository_path("platform-catalog/templates/qovery-cluster-v0/template.yaml"));
    assert_eq!(
        yaml_string(
            &template,
            &[
                "platformTemplateRelease",
                "runtimeSourceValues",
                "engineWorker.imageTagSuffix"
            ]
        ),
        None
    );

    let runtime_inputs = yaml_path(
        &template,
        &["platformTemplateRelease", "bootstrap", "component", "runtimeInputs"],
    )
    .and_then(serde_yaml::Value::as_sequence)
    .expect("bootstrap runtimeInputs must be a sequence");
    assert!(
        runtime_inputs
            .iter()
            .all(|input| yaml_string(input, &["name"]) != Some("engineWorker.imageTagSuffix"))
    );

    assert_eq!(
        yaml_string(
            &template,
            &[
                "platformTemplateRelease",
                "bootstrap",
                "component",
                "configRef",
                "evaluator",
                "kind"
            ],
        ),
        Some("PKL")
    );
    assert_eq!(
        yaml_string(
            &template,
            &[
                "platformTemplateRelease",
                "bootstrap",
                "component",
                "configRef",
                "evaluator",
                "entrypoint"
            ],
        ),
        Some("runtime-values/model.pkl")
    );

    let overlay = parse_yaml_file(repository_path(
        "platform-catalog/components/qovery-operator/config/static-values/overlays/qovery-demo.yaml",
    ));
    assert_eq!(
        yaml_string(&overlay, &["environmentVariables", "QOVERY_ENGINE_WORKER_IMAGE_TAG_SUFFIX"]),
        Some("-slim")
    );
    assert_eq!(
        yaml_string(&overlay, &["environmentVariables", "QOVERY_ENVIRONMENT_ENGINE_WORKER_PROFILE"]),
        Some("LOCAL_DEMO")
    );
}

#[test]
fn missing_referenced_config_fails_before_writing_a_template() {
    let mut fixture = RenderFixture::new();
    fixture.config_entries.remove(0);
    fixture.write_outputs();

    let output = fixture.render("0.1.0");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("configRef qovery-operator:v1 has no verified publication")
    );
    assert!(!fixture.destination.exists());
}

#[test]
fn wrong_chart_version_fails_complete_graph_validation() {
    let mut fixture = RenderFixture::new();
    let operator_chart = fixture
        .chart_entries
        .iter_mut()
        .find(|entry| entry["chart"].as_str() == Some("qovery-operator"))
        .expect("qovery-operator must be published");
    let expected_chart_version = operator_chart["version"]
        .as_str()
        .expect("chart version must be a string")
        .to_string();
    *operator_chart = chart_entry("qovery-operator", "9.9.9");
    fixture.write_outputs();

    let output = fixture.render("0.1.0");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        stderr.contains(&format!(
            "chart qovery-operator:{expected_chart_version} has no verified publication"
        )),
        "unexpected render error:\n{stderr}"
    );
}

#[test]
fn catalog_coordinate_must_match_the_template_identity() {
    let fixture = RenderFixture::new();
    fixture.write_outputs();

    let output = fixture.render("0.2.0");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("catalog expects qovery-cluster-v0:0.2.0"));
}

#[test]
fn karpenter_layer_is_optional_aws_byok_with_complete_customer_input_wiring() {
    let fixture = RenderFixture::new();
    fixture.write_outputs();
    assert!(fixture.render("0.1.0").status.success());
    let template: JsonValue = serde_yaml::from_str(&fs::read_to_string(&fixture.destination).unwrap()).unwrap();
    let layers = template["platformTemplateRelease"]["layers"].as_array().unwrap();
    let layer = layers
        .iter()
        .find(|layer| layer["key"] == "karpenter")
        .expect("Karpenter layer");
    assert_eq!(
        layer["applicability"],
        json!({"modes": ["CUSTOMER_MANAGED"], "providers": ["AWS"]})
    );
    assert_eq!(layer["mandatory"], false);
    assert_eq!(layer["enabledByDefault"], false);
    let components = layer["components"].as_array().unwrap();
    assert_eq!(
        components
            .iter()
            .map(|component| component["key"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["karpenter-crd", "karpenter", "karpenter-configuration"]
    );
    assert_eq!(
        components[1]["dependsOn"],
        json!([{"component":"karpenter-crd", "kind":"requires"}])
    );
    assert_eq!(
        components[2]["dependsOn"],
        json!([
            {"component":"karpenter-crd", "kind":"requires"}, {"component":"karpenter", "kind":"requires"}
        ])
    );
    for (component, expected_inputs) in [
        (
            &components[1],
            [
                "aws.eksClusterName",
                "aws.controllerRoleArn",
                "aws.interruptionQueueName",
            ],
        ),
        (
            &components[2],
            ["aws.eksClusterName", "aws.nodeRoleName", "aws.nodeSecurityGroupId"],
        ),
    ] {
        assert_eq!(
            component["configRef"]["evaluator"],
            json!({"kind":"PKL", "entrypoint":"runtime-values/model.pkl"})
        );
        let declarations = component["runtimeInputs"].as_array().unwrap();
        assert_eq!(
            declarations
                .iter()
                .map(|input| input["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected_inputs
        );
        for input in declarations {
            assert_eq!(
                input["sources"],
                json!([{"kind":"customerProvidedValue", "modes":["CUSTOMER_MANAGED"]}])
            );
            assert_eq!(input["required"], true);
        }
    }
    let demo: JsonValue = serde_yaml::from_str(
        &fs::read_to_string(repository_path("platform-catalog/templates/qovery-demo-v0/template.yaml")).unwrap(),
    )
    .unwrap();
    assert!(
        !demo["platformTemplateRelease"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|layer| layer["key"] == "karpenter")
    );
}

#[test]
fn platform_workloads_wait_for_optional_karpenter_without_blocking_its_bootstrap() {
    let template: JsonValue = serde_yaml::from_str(
        &fs::read_to_string(repository_path("platform-catalog/templates/qovery-cluster-v0/template.yaml")).unwrap(),
    )
    .unwrap();
    let release = &template["platformTemplateRelease"];
    let workloads = [
        "cluster-agent",
        "shell-agent",
        "loki",
        "alloy",
        "cert-manager",
        "qovery-cert-manager-webhook",
        "external-dns",
    ];
    let other_components = [
        "qovery-priority-class",
        "external-dns-secret",
        "cert-manager-configs",
        "karpenter-crd",
        "karpenter",
        "karpenter-configuration",
    ];
    for component in release["layers"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|layer| layer["components"].as_array().unwrap())
    {
        let key = component["key"].as_str().unwrap();
        let dependencies = component["dependsOn"].as_array().map(Vec::as_slice).unwrap_or_default();
        if workloads.contains(&key) {
            let edges: Vec<_> = dependencies
                .iter()
                .filter(|edge| edge["component"] == "karpenter-configuration")
                .collect();
            assert_eq!(
                edges,
                [&json!({"component": "karpenter-configuration", "kind": "after"})],
                "{key} must wait only when Karpenter is enabled"
            );
        } else {
            assert!(
                other_components.contains(&key),
                "classify new component {key}: does it deploy pods that need Karpenter capacity?"
            );
            assert!(
                dependencies
                    .iter()
                    .all(|edge| edge["component"] != "karpenter-configuration"),
                "{key} does not deploy dependent pods and must not wait for Karpenter configuration"
            );
            if key.starts_with("karpenter") {
                assert!(
                    dependencies
                        .iter()
                        .all(|edge| !workloads.contains(&edge["component"].as_str().unwrap())),
                    "{key} must not depend on workloads that wait for it"
                );
            }
        }
    }
    assert!(
        release["bootstrap"]["component"]["dependsOn"].is_null(),
        "the operator must bootstrap independently of Karpenter"
    );
}
