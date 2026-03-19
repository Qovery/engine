use anyhow::{Context, Result};
use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use std::io::{self, Read, Write};

const MANDATORY_DEPLOYED_BY_LABEL_KEY: &str = "deployed-by";
const MANDATORY_DEPLOYED_BY_LABEL_VALUE: &str = "qovery";

pub fn main() -> Result<()> {
    run_post_renderer_labels(io::stdin(), io::stdout()).context("post-renderer failed")
}

fn run_post_renderer_labels<R: Read, W: Write>(mut input: R, mut output: W) -> Result<()> {
    let mut manifest = String::new();
    input.read_to_string(&mut manifest).context("cannot read stdin")?;

    if manifest.trim().is_empty() {
        return Ok(());
    }

    let final_output = match render_post_renderer_labels(&manifest) {
        Ok(rendered) => rendered,
        Err(e) => {
            eprintln!("post-renderer labels failed, passing through unmodified manifest: {e}");
            manifest
        }
    };

    output
        .write_all(final_output.as_bytes())
        .context("cannot write stdout")?;

    Ok(())
}

fn render_post_renderer_labels(manifest: &str) -> Result<String> {
    let mut docs = Vec::new();
    for doc in serde_yaml::Deserializer::from_str(manifest) {
        let value = Value::deserialize(doc).context("invalid YAML manifest")?;
        docs.push(value);
    }

    for doc in &mut docs {
        add_mandatory_deployed_by_label(doc);
    }

    let mut rendered_docs = Vec::with_capacity(docs.len());
    for doc in docs {
        let yaml = serde_yaml::to_string(&doc).context("cannot serialize YAML manifest")?;
        rendered_docs.push(yaml.trim_end().to_string());
    }

    Ok(rendered_docs.join("\n---\n"))
}

fn add_mandatory_deployed_by_label(doc: &mut Value) {
    if has_helm_hook_annotation(doc) {
        return;
    }

    let Some(root) = doc.as_mapping_mut() else {
        return;
    };

    let metadata = ensure_mapping_field(root, "metadata");
    let labels = ensure_mapping_field(metadata, "labels");
    labels.insert(
        Value::String(MANDATORY_DEPLOYED_BY_LABEL_KEY.to_string()),
        Value::String(MANDATORY_DEPLOYED_BY_LABEL_VALUE.to_string()),
    );
}

fn has_helm_hook_annotation(doc: &Value) -> bool {
    let Some(root) = doc.as_mapping() else {
        return false;
    };
    let Some(metadata) = root
        .get(Value::String("metadata".to_string()))
        .and_then(Value::as_mapping)
    else {
        return false;
    };
    let Some(annotations) = metadata
        .get(Value::String("annotations".to_string()))
        .and_then(Value::as_mapping)
    else {
        return false;
    };

    annotations.contains_key(Value::String("helm.sh/hook".to_string()))
}

fn ensure_mapping_field<'a>(map: &'a mut Mapping, key: &str) -> &'a mut Mapping {
    let key_value = Value::String(key.to_string());
    let value = map.entry(key_value).or_insert_with(|| Value::Mapping(Mapping::new()));
    if !value.is_mapping() {
        *value = Value::Mapping(Mapping::new());
    }

    value.as_mapping_mut().expect("value must be a mapping")
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use serde::Deserialize;
    use serde_yaml::Value;

    use super::run_post_renderer_labels;

    fn parse_yaml_docs(yaml: &str) -> Vec<Value> {
        serde_yaml::Deserializer::from_str(yaml)
            .map(|doc| Value::deserialize(doc).expect("cannot deserialize YAML document"))
            .collect()
    }

    #[test]
    fn test_post_renderer_labels_adds_label_and_skips_hooks() {
        let input = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: demo
spec: {}
---
apiVersion: batch/v1
kind: Job
metadata:
  name: hook-job
  annotations:
    helm.sh/hook: pre-install
spec: {}
"#;

        let mut output: Vec<u8> = Vec::new();
        run_post_renderer_labels(Cursor::new(input), &mut output).expect("post renderer should succeed");

        let output_yaml = String::from_utf8(output).expect("output must be valid utf8");
        let docs = parse_yaml_docs(&output_yaml);
        assert_eq!(docs.len(), 2);

        let deployment_labels = docs[0]
            .as_mapping()
            .and_then(|doc| doc.get(Value::String("metadata".to_string())))
            .and_then(Value::as_mapping)
            .and_then(|metadata| metadata.get(Value::String("labels".to_string())))
            .and_then(Value::as_mapping)
            .expect("deployment labels should exist");
        assert_eq!(
            deployment_labels.get(Value::String("deployed-by".to_string())),
            Some(&Value::String("qovery".to_string()))
        );

        let hook_labels = docs[1]
            .as_mapping()
            .and_then(|doc| doc.get(Value::String("metadata".to_string())))
            .and_then(Value::as_mapping)
            .and_then(|metadata| metadata.get(Value::String("labels".to_string())))
            .and_then(Value::as_mapping);
        assert!(hook_labels.is_none(), "hook resources must not be mutated");
    }

    #[test]
    fn test_post_renderer_labels_passes_through_invalid_yaml() {
        let input = "apiVersion: v1\nkind: ConfigMap\nmetadata: [\n";

        let mut output: Vec<u8> = Vec::new();
        let ret = run_post_renderer_labels(Cursor::new(input), &mut output);

        assert!(ret.is_ok(), "post renderer should be best-effort");
        let output_yaml = String::from_utf8(output).expect("output must be valid utf8");
        assert_eq!(output_yaml, input);
    }
}
