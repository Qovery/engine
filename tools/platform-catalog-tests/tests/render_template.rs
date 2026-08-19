use platform_catalog_tests::{
    REGISTRY, contains_string, mapping_string, mappings_for_key, parse_yaml_file, repository_path, run, yaml_path,
    yaml_string,
};
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const CHART_DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Deserialize)]
struct Catalog {
    components: Vec<CatalogComponent>,
    charts: Vec<CatalogChart>,
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
        run(Command::new(repository_path("scripts/publish-platform-catalog.sh")).args([
            "render",
            repository_path("platform-catalog/templates/qovery-cluster-v0/template.yaml")
                .to_str()
                .expect("repository path must be UTF-8"),
            self.config_output.to_str().expect("temporary path must be UTF-8"),
            self.chart_output.to_str().expect("temporary path must be UTF-8"),
            self.destination.to_str().expect("temporary path must be UTF-8"),
            "qovery-cluster-v0",
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
    let mut config_references = Vec::new();
    mappings_for_key(&rendered, "configRef", &mut config_references);

    for entry in &fixture.config_entries {
        let component = entry["component"].as_str().expect("component must be a string");
        let version = entry["version"].as_str().expect("version must be a string");
        let digest = entry["digest"].as_str().expect("digest must be a string");
        assert!(
            config_references.iter().any(|reference| {
                mapping_string(reference, "chart") == Some(component)
                    && mapping_string(reference, "version") == Some(version)
                    && mapping_string(reference, "digest") == Some(digest)
            }),
            "missing rendered config reference {component}:{version}@{digest}"
        );
    }
    assert!(contains_string(&rendered, "oci://public.ecr.aws/r3m4q3r9/charts/"));
}

#[test]
fn every_component_release_uses_the_protected_qovery_namespace() {
    let template = parse_yaml_file(repository_path("platform-catalog/templates/qovery-cluster-v0/template.yaml"));
    let mut releases = Vec::new();
    mappings_for_key(&template, "release", &mut releases);

    assert!(!releases.is_empty(), "platform template must declare component releases");
    for release in releases {
        assert_eq!(
            mapping_string(release, "namespace"),
            Some("qovery"),
            "every catalog component release must stay inside q-core's protected namespace"
        );
    }
}

#[test]
fn engine_worker_image_tag_suffix_defaults_to_empty_and_comes_from_qcore() {
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
        Some("")
    );

    let runtime_inputs = yaml_path(
        &template,
        &["platformTemplateRelease", "bootstrap", "component", "runtimeInputs"],
    )
    .and_then(serde_yaml::Value::as_sequence)
    .expect("bootstrap runtimeInputs must be a sequence");
    let suffix_input = runtime_inputs
        .iter()
        .find(|input| yaml_string(input, &["name"]) == Some("engineWorker.imageTagSuffix"))
        .expect("engineWorker.imageTagSuffix runtime input must be declared");

    assert_eq!(yaml_string(suffix_input, &["source", "kind"]), Some("qcoreValue"));
    assert_eq!(
        yaml_string(suffix_input, &["source", "key"]),
        Some("engineWorker.imageTagSuffix")
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
