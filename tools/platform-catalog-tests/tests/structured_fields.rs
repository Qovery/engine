//! Wire compatibility and invalid-descriptor checks for the unpublished A1 exchange fixture.

use platform_catalog_tests::{assert_success, repository_path, run};
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

const FIXTURE: &str = "platform-catalog/pkl/tests/fixtures/karpenter-v1";

fn pkl() -> Command {
    Command::new(env::var_os("PKL_BIN").unwrap_or_else(|| "pkl".into()))
}

fn copy_bundle(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("bundle directory must be writable");
    for entry in fs::read_dir(source).expect("bundle must be readable") {
        let entry = entry.expect("bundle entry must be readable");
        let destination = target.join(entry.file_name());
        if entry.path().is_dir() {
            copy_bundle(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("bundle module must be copied");
        }
    }
}

#[test]
fn structured_exchanges_run_as_an_isolated_bundle_through_the_existing_entrypoint() {
    let fixture = repository_path(FIXTURE);
    let isolated = tempdir().expect("temporary bundle must be created");
    copy_bundle(&fixture.join("config/runtime-values"), isolated.path());

    for case in [
        "DESCRIBE",
        "RESOLVE_REQUIREMENTS",
        "VALIDATE",
        "COMPILE",
        "invalid-VALIDATE",
        "invalid-COMPILE",
    ] {
        let request =
            fs::read_to_string(fixture.join(format!("{case}.request.json"))).expect("request fixture must exist");
        let expected = fs::read(fixture.join(format!("{case}.response.json"))).expect("response fixture must exist");
        let output = assert_success(
            pkl()
                .arg("eval")
                .arg("--root-dir")
                .arg(isolated.path())
                .arg("--allowed-modules=^pkl:,^file:")
                .arg("--allowed-resources=^prop:request$")
                .arg("--no-cache")
                .arg("-p")
                .arg(format!("request={request}"))
                .arg(isolated.path().join("model.pkl")),
        );
        assert_eq!(output.stdout, expected, "{case} exchange bytes changed");
        let response: Value = serde_json::from_slice(&output.stdout).expect("response must be JSON");
        if case != "COMPILE" {
            assert!(response.get("helmValues").is_none(), "{case} must omit helmValues");
        }
        if case.starts_with("invalid-") {
            let violations = response["violations"].as_array().expect("violations must be an array");
            assert_eq!(violations.len(), 3);
            assert!(
                violations
                    .iter()
                    .any(|entry| entry["fieldPath"] == "nodePools[1].ami.id")
            );
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScalarExchange {
    request: Value,
    response: Value,
}

#[test]
fn scalar_exchanges_match_the_unmodified_base_in_all_four_operations() {
    let source = fs::read(repository_path("platform-catalog/pkl/tests/fixtures/scalar-v1/exchanges.json"))
        .expect("scalar baseline must exist");
    let exchanges: Vec<ScalarExchange> = serde_json::from_slice(&source).expect("scalar baseline must be valid");
    assert_eq!(exchanges.len(), 16, "all captured scalar cases must remain covered");
    for exchange in exchanges {
        let component = exchange.request["componentKey"]
            .as_str()
            .expect("component key must be a string");
        let model = repository_path(format!(
            "platform-catalog/components/{component}/config/runtime-values/model.pkl"
        ));
        let output = assert_success(
            pkl()
                .arg("eval")
                .arg("-p")
                .arg(format!("request={}", exchange.request))
                .arg(model),
        );
        let result: Value = serde_json::from_slice(&output.stdout).expect("scalar output must be valid JSON");
        assert_eq!(result, exchange.response, "scalar exchange changed: {}", exchange.request);
    }
}

fn reject_descriptor(body: &str, diagnostic: &str) {
    let directory = tempdir().expect("temporary module directory must be created");
    let module = directory.path().join("invalid.pkl");
    let contract = repository_path("platform-catalog/pkl/contract.pkl");
    fs::write(
        &module,
        format!(
            "import \"{}\" as contract\n{body}\noutput {{ value = sample; renderer = new JsonRenderer {{}} }}\n",
            contract.display()
        ),
    )
    .expect("invalid module must be writable");
    let output = run(pkl().arg("eval").arg(module));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "invalid descriptor was accepted: {body}");
    assert!(stderr.contains(diagnostic), "unexpected failure for {body}:\n{stderr}");
}

#[test]
fn contract_rejects_contradictory_shapes_and_preserves_scalar_logical_inputs() {
    for (body, diagnostic) in [
        (
            r#"sample = new contract.Field { key = "x"; label = "x"; required = false; type = "array" }"#,
            "Expected value of type",
        ),
        (
            r#"sample = new contract.LogicalInput { key = "x"; label = "x"; scope = "CLUSTER"; type = "object" }"#,
            "Expected value of type",
        ),
        (
            r#"sample = new contract.Constraints { minItems = 1 }"#,
            "Cannot find property `minItems`",
        ),
        (
            r#"sample = new contract.ObjectField { key = "x"; label = "x"; required = false; fields = List(); items = new contract.ScalarItem { type = "string" } }"#,
            "Cannot find property `items`",
        ),
        (
            r#"sample = new contract.ArrayField { key = "x"; label = "x"; required = false; items = new contract.ScalarItem { type = "object" } }"#,
            "Expected value of type",
        ),
        (
            r#"sample = new contract.ArrayField { key = "x"; label = "x"; required = false; items = new contract.ObjectItem { fields = List() } }"#,
            "Type constraint",
        ),
        (
            r#"sample = new contract.ArrayField { key = "x"; label = "x"; required = false; items = new contract.ScalarItem { type = "string" }; itemFields = List(List()) }"#,
            "Type constraint",
        ),
        (
            r#"sample = new contract.CollectionConstraints { minItems = -1 }"#,
            "Type constraint",
        ),
        (
            r#"sample = new contract.CollectionConstraints { minItems = 2; maxItems = 1 }"#,
            "Type constraint",
        ),
    ] {
        reject_descriptor(body, diagnostic);
    }
}

#[test]
fn contract_rejects_ambiguous_nested_keys_and_nested_sensitive_descriptors() {
    let scalar = r#"new contract.Field { key = "x"; type = "string"; required = false; label = "x" }"#;
    for (children, diagnostic) in [
        (format!("List({scalar}, {scalar})"), "Type constraint"),
        (format!("List(({scalar}) {{ key = \"x.y\" }})"), "Type constraint"),
        (format!("List(({scalar}) {{ sensitive = true }})"), "Type constraint"),
    ] {
        reject_descriptor(
            &format!(
                r#"sample = new contract.ObjectField {{ key = "root"; label = "root"; required = false; sensitive = true; fields = {children} }}"#
            ),
            diagnostic,
        );
    }
    // Both prototypes and evaluated rows enforce the nested-secret rule.
    for property in ["items", "itemFields"] {
        let fields = format!("List(({scalar}) {{ sensitive = true }})");
        let (prototype, rows) = if property == "items" {
            (fields, "List()".to_owned())
        } else {
            ("List()".to_owned(), format!("List({fields})"))
        };
        reject_descriptor(
            &format!(
                r#"sample = new contract.ArrayField {{ key = "rows"; label = "rows"; required = false; items = new contract.ObjectItem {{ fields = {prototype} }}; itemFields = {rows} }}"#
            ),
            "Type constraint",
        );
    }
}
