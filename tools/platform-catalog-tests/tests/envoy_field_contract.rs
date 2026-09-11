//! Exercise Envoy descriptors through the isolated JSON entrypoint consumed by q-core.
use platform_catalog_tests::{assert_success, repository_path};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn copy_bundle(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.path().is_dir() {
            copy_bundle(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

fn evaluate(component: &str, operation: &str, config: Value) -> Value {
    let isolated = tempdir().unwrap();
    copy_bundle(
        &repository_path(format!("platform-catalog/components/{component}/config/runtime-values")),
        isolated.path(),
    );
    let request = json!({"operation":operation, "componentKey":component, "profileConfig":config,
        "clusterInputs":{"dns.managedDomain":"example.com"}, "clusterContext":null, "enabledComponents":[]});
    let output = assert_success(Command::new("pkl").args([
        "eval",
        "--root-dir",
        isolated.path().to_str().unwrap(),
        "--allowed-modules=^pkl:,^file:",
        "--allowed-resources=^prop:request$",
        "--no-cache",
        "-p",
        &format!("request={request}"),
        isolated.path().join("model.pkl").to_str().unwrap(),
    ]));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn certificate_array_uses_shared_object_items_and_row_descriptors() {
    for operation in ["DESCRIBE", "RESOLVE_REQUIREMENTS", "VALIDATE", "COMPILE"] {
        for rows in [
            json!([]),
            json!([{"name":"first","ca_crt":"CA1"},{"name":"second","ca_crt":"CA2"}]),
        ] {
            let result = evaluate(
                "qovery-cluster-gateway",
                operation,
                json!({"envoy.client_validation.ca_certificates":rows}),
            );
            assert_eq!(result["violations"], json!([]));
            let field = result["fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["key"] == "envoy.client_validation.ca_certificates")
                .unwrap();
            assert_eq!(field["type"], "array");
            assert_eq!(field["constraints"]["maxItems"], 8);
            assert_eq!(field["items"]["type"], "object");
            let members = field["items"]["fields"]
                .as_array()
                .expect("shared ObjectItem exposes fields, not properties");
            assert_eq!(
                members.iter().map(|m| m["key"].as_str().unwrap()).collect::<Vec<_>>(),
                ["name", "ca_crt"]
            );
            assert!(field["items"].get("properties").is_none());
            let descriptors = field["itemFields"]
                .as_array()
                .expect("object arrays carry row descriptors");
            assert_eq!(descriptors.len(), rows.as_array().unwrap().len());
            for descriptor in descriptors {
                assert_eq!(descriptor, &field["items"]["fields"]);
            }
            if operation == "COMPILE" {
                let expected = rows
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| {
                        json!({
                    "name":format!("envoy-client-validation-{}",row["name"].as_str().unwrap()),
                    "namespace":"qovery", "caCrt":row["ca_crt"]})
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    result["helmValues"]["gateway"]["qoveryPublic"]["clientValidation"]["caCertificates"],
                    json!(expected)
                );
            }
        }
    }
}

#[test]
fn scalar_descriptors_do_not_gain_collection_properties() {
    for component in [
        "cert-manager-configs",
        "envoy-gateway",
        "qovery-gateway-class",
        "qovery-cluster-gateway",
    ] {
        let result = evaluate(component, "DESCRIBE", json!({}));
        for field in result["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["type"] != "array")
        {
            for key in ["items", "properties", "itemFields", "fields"] {
                assert!(field.get(key).is_none(), "{component}: scalar gained {key}: {field}");
            }
            for key in ["minItems", "maxItems"] {
                assert!(field["constraints"].get(key).is_none());
            }
        }
    }
}

#[test]
fn certificate_validation_uses_indexed_shared_errors_and_blocks_compile() {
    for (rows, code, path) in [
        (
            json!([{"name":"partner","ca_crt":"CA","typo":true}]),
            "UNKNOWN_FIELD",
            "[0].typo",
        ),
        (json!([{"name":"partner"}]), "REQUIRED_FIELD_MISSING", "[0].ca_crt"),
        (json!(["not-an-object"]), "INVALID_TYPE", "[0]"),
        (json!([{"name":42,"ca_crt":"CA"}]), "INVALID_TYPE", "[0].name"),
    ] {
        let result = evaluate(
            "qovery-cluster-gateway",
            "COMPILE",
            json!({"envoy.client_validation.ca_certificates":rows}),
        );
        let expected_path = format!("envoy.client_validation.ca_certificates{path}");
        assert!(
            result["violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["code"] == code && v["fieldPath"] == expected_path),
            "{result}"
        );
        assert!(result["helmValues"].is_null(), "invalid certificates must never compile");
    }
}
