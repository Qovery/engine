use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const REGISTRY: &str = "public.ecr.aws/r3m4q3r9";

pub fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("platform catalog test crate must live below the repository root")
}

pub fn repository_path(path: impl AsRef<Path>) -> PathBuf {
    repository_root().join(path)
}

pub fn run(command: &mut Command) -> Output {
    let description = format!("{command:?}");
    command
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {description}: {error}"))
}

#[track_caller]
pub fn assert_success(command: &mut Command) -> Output {
    let description = format!("{command:?}");
    let output = run(command);
    assert!(
        output.status.success(),
        "command {description} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

pub fn helm_template(
    release: &str,
    chart: &str,
    namespace: &str,
    value_files: &[PathBuf],
    extra_arguments: &[&str],
) -> String {
    let mut command = Command::new("helm");
    command
        .arg("template")
        .arg(release)
        .arg(repository_path(chart))
        .arg("--namespace")
        .arg(namespace);
    for value_file in value_files {
        command.arg("--values").arg(value_file);
    }
    command.args(extra_arguments);

    String::from_utf8(assert_success(&mut command).stdout).expect("helm output must be UTF-8")
}

pub fn parse_yaml_documents(source: &str) -> Vec<Value> {
    serde_yaml::Deserializer::from_str(source)
        .map(|document| Value::deserialize(document).expect("rendered YAML must be valid"))
        .filter(|document| !document.is_null())
        .collect()
}

pub fn parse_yaml_file(path: impl AsRef<Path>) -> Value {
    let content = std::fs::read_to_string(path.as_ref())
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.as_ref().display()));
    serde_yaml::from_str(&content)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.as_ref().display()))
}

pub fn yaml_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |current, key| {
        current.as_mapping()?.get(Value::String((*key).to_owned()))
    })
}

pub fn yaml_string<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    yaml_path(value, path)?.as_str()
}

pub fn mapping_string<'a>(mapping: &'a Mapping, key: &str) -> Option<&'a str> {
    mapping.get(Value::String(key.to_owned()))?.as_str()
}

pub fn document_kind(document: &Value) -> Option<&str> {
    yaml_string(document, &["kind"])
}

pub fn document_name(document: &Value) -> Option<&str> {
    yaml_string(document, &["metadata", "name"])
}

pub fn document_by_kind_and_name<'a>(documents: &'a [Value], kind: &str, name: &str) -> Option<&'a Value> {
    documents
        .iter()
        .find(|document| document_kind(document) == Some(kind) && document_name(document) == Some(name))
}

pub fn contains_key(value: &Value, expected: &str) -> bool {
    match value {
        Value::Mapping(mapping) => mapping.iter().any(|(key, value)| {
            key.as_str() == Some(expected) || contains_key(key, expected) || contains_key(value, expected)
        }),
        Value::Sequence(sequence) => sequence.iter().any(|value| contains_key(value, expected)),
        Value::Tagged(tagged) => contains_key(&tagged.value, expected),
        _ => false,
    }
}

pub fn contains_string(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(string) => string == expected,
        Value::Mapping(mapping) => mapping
            .iter()
            .any(|(key, value)| contains_string(key, expected) || contains_string(value, expected)),
        Value::Sequence(sequence) => sequence.iter().any(|value| contains_string(value, expected)),
        Value::Tagged(tagged) => contains_string(&tagged.value, expected),
        _ => false,
    }
}

pub fn contains_string_fragment(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(string) => string.contains(expected),
        Value::Mapping(mapping) => mapping
            .iter()
            .any(|(key, value)| contains_string_fragment(key, expected) || contains_string_fragment(value, expected)),
        Value::Sequence(sequence) => sequence.iter().any(|value| contains_string_fragment(value, expected)),
        Value::Tagged(tagged) => contains_string_fragment(&tagged.value, expected),
        _ => false,
    }
}

pub fn mappings_for_key<'a>(value: &'a Value, expected: &str, result: &mut Vec<&'a Mapping>) {
    match value {
        Value::Mapping(mapping) => {
            for (key, value) in mapping {
                if key.as_str() == Some(expected)
                    && let Some(nested_mapping) = value.as_mapping()
                {
                    result.push(nested_mapping);
                }
                mappings_for_key(value, expected, result);
            }
        }
        Value::Sequence(sequence) => {
            for value in sequence {
                mappings_for_key(value, expected, result);
            }
        }
        Value::Tagged(tagged) => mappings_for_key(&tagged.value, expected, result),
        _ => {}
    }
}
