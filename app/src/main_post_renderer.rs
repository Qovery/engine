use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use std::io::{self, Read, Write};

const MANDATORY_DEPLOYED_BY_LABEL_KEY: &str = "deployed-by";
const MANDATORY_DEPLOYED_BY_LABEL_VALUE: &str = "qovery";
const PUBLIC_ECR_IMAGE_PREFIX: &str = "public.ecr.aws/";
const CONTAINER_FIELDS: [&str; 3] = ["containers", "initContainers", "ephemeralContainers"];

#[derive(Parser)]
struct PostRendererArgs {
    /// Private ECR registry prefix mirroring public.ecr.aws through pull-through cache.
    #[arg(long)]
    public_ecr_image_mirror: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct PostRendererConfig {
    public_ecr_image_mirror: Option<String>,
}

pub fn main() -> Result<()> {
    let args = PostRendererArgs::parse();
    let config = PostRendererConfig {
        public_ecr_image_mirror: args.public_ecr_image_mirror,
    };

    run_post_renderer(io::stdin(), io::stdout(), &config).context("post-renderer failed")
}

fn run_post_renderer<R: Read, W: Write>(mut input: R, mut output: W, config: &PostRendererConfig) -> Result<()> {
    let mut manifest = String::new();
    input.read_to_string(&mut manifest).context("cannot read stdin")?;

    if manifest.trim().is_empty() {
        return Ok(());
    }

    let final_output = match render_post_renderer(&manifest, config) {
        Ok(rendered) => rendered,
        Err(e) => {
            eprintln!("post-renderer failed, passing through unmodified manifest: {e}");
            manifest
        }
    };

    output
        .write_all(final_output.as_bytes())
        .context("cannot write stdout")?;

    Ok(())
}

fn render_post_renderer(manifest: &str, config: &PostRendererConfig) -> Result<String> {
    let public_ecr_image_mirror = config
        .public_ecr_image_mirror
        .as_deref()
        .map(|mirror| mirror.trim_end_matches('/'));
    let mut rendered = String::with_capacity(manifest.len());

    for doc in serde_yaml::Deserializer::from_str(manifest) {
        let mut value = Value::deserialize(doc).context("invalid YAML manifest")?;

        add_mandatory_deployed_by_label(&mut value);
        if let Some(public_ecr_image_mirror) = public_ecr_image_mirror {
            rewrite_public_ecr_container_images(&mut value, public_ecr_image_mirror);
        }

        if !rendered.is_empty() {
            rendered.push_str("\n---\n");
        }
        let yaml = serde_yaml::to_string(&value).context("cannot serialize YAML manifest")?;
        rendered.push_str(yaml.trim_end());
    }

    Ok(rendered)
}

fn rewrite_public_ecr_container_images(value: &mut Value, public_ecr_image_mirror: &str) {
    match value {
        Value::Mapping(mapping) => {
            for container_field in CONTAINER_FIELDS {
                let Some(containers) = mapping.get_mut(container_field).and_then(Value::as_sequence_mut) else {
                    continue;
                };

                for container in containers {
                    let Some(Value::String(image)) = container
                        .as_mapping_mut()
                        .and_then(|container| container.get_mut("image"))
                    else {
                        continue;
                    };

                    let Some(public_ecr_image_path) = image.strip_prefix(PUBLIC_ECR_IMAGE_PREFIX) else {
                        continue;
                    };

                    let mirrored_image = format!("{public_ecr_image_mirror}/{public_ecr_image_path}");
                    *image = mirrored_image;
                }
            }

            for child in mapping.values_mut() {
                rewrite_public_ecr_container_images(child, public_ecr_image_mirror);
            }
        }
        Value::Sequence(sequence) => {
            for child in sequence {
                rewrite_public_ecr_container_images(child, public_ecr_image_mirror);
            }
        }
        _ => {}
    }
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
    let Some(metadata) = root.get("metadata").and_then(Value::as_mapping) else {
        return false;
    };
    let Some(annotations) = metadata.get("annotations").and_then(Value::as_mapping) else {
        return false;
    };

    annotations.contains_key("helm.sh/hook")
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

    use super::{PostRendererConfig, run_post_renderer};

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
        run_post_renderer(Cursor::new(input), &mut output, &PostRendererConfig::default())
            .expect("post renderer should succeed");

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
        let input = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: valid\n---\napiVersion: v1\nkind: ConfigMap\nmetadata: [\n";

        let mut output: Vec<u8> = Vec::new();
        let ret = run_post_renderer(Cursor::new(input), &mut output, &PostRendererConfig::default());

        assert!(ret.is_ok(), "post renderer should be best-effort");
        let output_yaml = String::from_utf8(output).expect("output must be valid utf8");
        assert_eq!(output_yaml, input);
    }

    #[test]
    fn test_post_renderer_rewrites_only_public_ecr_container_images() {
        let input = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: demo
spec:
  template:
    spec:
      initContainers:
        - name: init
          image: public.ecr.aws/qovery/init:v1
      containers:
        - name: qovery
          image: public.ecr.aws/r3m4q3r9/cluster-agent@sha256:abc
        - name: external
          image: docker.io/library/nginx:latest
"#;
        let config = PostRendererConfig {
            public_ecr_image_mirror: Some(
                "123456789012.dkr.ecr.eu-west-3.amazonaws.com/qovery-ecr-public/".to_string(),
            ),
        };
        let mut output = Vec::new();

        run_post_renderer(Cursor::new(input), &mut output, &config).expect("post renderer should succeed");

        let output_yaml = String::from_utf8(output).expect("output must be valid utf8");
        assert!(output_yaml.contains("123456789012.dkr.ecr.eu-west-3.amazonaws.com/qovery-ecr-public/qovery/init:v1"));
        assert!(output_yaml.contains(
            "123456789012.dkr.ecr.eu-west-3.amazonaws.com/qovery-ecr-public/r3m4q3r9/cluster-agent@sha256:abc"
        ));
        assert!(output_yaml.contains("docker.io/library/nginx:latest"));
        assert!(!output_yaml.contains("public.ecr.aws/"));
    }

    #[test]
    fn test_post_renderer_rewrites_hook_and_nested_cron_job_images() {
        let input = r#"
apiVersion: batch/v1
kind: CronJob
metadata:
  name: hook-job
  annotations:
    helm.sh/hook: pre-install
spec:
  jobTemplate:
    spec:
      template:
        spec:
          containers:
            - name: job
              image: public.ecr.aws/qovery/job:v1
"#;
        let config = PostRendererConfig {
            public_ecr_image_mirror: Some("123456789012.dkr.ecr.eu-west-3.amazonaws.com/qovery-ecr-public".to_string()),
        };
        let mut output = Vec::new();

        run_post_renderer(Cursor::new(input), &mut output, &config).expect("post renderer should succeed");

        let output_yaml = String::from_utf8(output).expect("output must be valid utf8");
        assert!(output_yaml.contains("123456789012.dkr.ecr.eu-west-3.amazonaws.com/qovery-ecr-public/qovery/job:v1"));
        let docs = parse_yaml_docs(&output_yaml);
        let hook_labels = docs[0]
            .as_mapping()
            .and_then(|doc| doc.get(Value::String("metadata".to_string())))
            .and_then(Value::as_mapping)
            .and_then(|metadata| metadata.get(Value::String("labels".to_string())))
            .and_then(Value::as_mapping);
        assert!(hook_labels.is_none(), "hook resources must not receive labels");
    }

    #[test]
    fn test_post_renderer_keeps_public_ecr_images_when_mirror_is_disabled() {
        let input = r#"
apiVersion: v1
kind: Pod
metadata:
  name: demo
spec:
  containers:
    - name: qovery
      image: public.ecr.aws/qovery/demo:v1
"#;
        let mut output = Vec::new();

        run_post_renderer(Cursor::new(input), &mut output, &PostRendererConfig::default())
            .expect("post renderer should succeed");

        let output_yaml = String::from_utf8(output).expect("output must be valid utf8");
        assert!(output_yaml.contains("public.ecr.aws/qovery/demo:v1"));
    }
}
