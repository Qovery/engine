use platform_catalog_tests::{assert_success, repository_path};
use serde_json::{Value, json};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn collect_runtime_models(directory: &Path, models: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(directory).unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
    {
        let path = entry.expect("component directory entry must be readable").path();
        if path.is_dir() {
            collect_runtime_models(&path, models);
        } else if path.file_name() == Some(OsStr::new("model.pkl"))
            && path.parent().and_then(Path::file_name) == Some(OsStr::new("runtime-values"))
        {
            models.push(path);
        }
    }
}

#[test]
fn every_executable_model_renders_the_shared_describe_envelope() {
    let components_directory = repository_path("platform-catalog/components");
    let mut models = Vec::new();
    collect_runtime_models(&components_directory, &mut models);
    models.sort();
    assert!(!models.is_empty(), "at least one executable platform model must exist");

    let pkl_binary = env::var_os("PKL_BIN").unwrap_or_else(|| "pkl".into());
    for model in models {
        let relative_model = model
            .strip_prefix(&components_directory)
            .expect("runtime model must be below the components directory");
        let component_key = relative_model
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .expect("component key must be valid UTF-8");
        let request = json!({
            "operation": "DESCRIBE",
            "componentKey": component_key,
            "profileConfig": {},
            "clusterContext": null,
            "clusterInputs": {},
            "componentOutputs": {},
        });

        let mut command = Command::new(&pkl_binary);
        command
            .arg("eval")
            .arg("-p")
            .arg(format!("request={request}"))
            .arg(&model);
        let output = assert_success(&mut command);
        let result: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{} did not render valid JSON: {error}", model.display()));

        let envelope = result
            .as_object()
            .unwrap_or_else(|| panic!("{} must render a JSON object", model.display()));
        for field in ["fields", "requiredInputs", "violations"] {
            assert!(
                envelope.get(field).is_some_and(Value::is_array),
                "{} must render {field} as an array",
                model.display()
            );
        }
        assert!(
            !envelope.contains_key("helmValues"),
            "{} must omit helmValues from DESCRIBE instead of rendering null",
            model.display()
        );
    }
}
