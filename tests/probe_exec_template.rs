use qovery_engine::tera_utils::render_one_off;
use serde_json::json;
use tera::Context;

/// Minimal template snippet that mirrors the exec probe rendering in
/// `q-container/templates/deployment.j2.yaml` (and statefulset, job, cronjob).
///
/// The template iterates over each command element and emits it as a quoted YAML
/// scalar.  This guarantees that strings containing commas, semicolons, quotes,
/// newlines or other special characters are never split or mangled by the YAML
/// parser, nor able to add fields to the surrounding manifest.
const PROBE_EXEC_TEMPLATE: &str = r#"
{%- if service.liveness_probe.type.exec %}
            exec:
              command:
              {%- for cmd in service.liveness_probe.type.exec.commands %}
              - {{ cmd | yaml_encode }}
              {%- endfor %}
{%- endif %}
"#;

fn render_exec_probe(commands: Vec<&str>) -> String {
    let mut context = Context::new();
    context.insert(
        "service",
        &json!({
            "liveness_probe": {
                "type": {
                    "exec": {
                        "commands": commands,
                    }
                }
            }
        }),
    );

    render_one_off(PROBE_EXEC_TEMPLATE, &context).expect("exec probe template should render")
}

#[test]
fn exec_probe_preserves_commands_with_commas() {
    let rendered = render_exec_probe(vec![
        "python",
        "-c",
        "import os,time;f='/code/celerybeat-liveness';assert os.path.exists(f) and time.time()-os.path.getmtime(f)<240",
    ]);

    // Parse the rendered YAML and verify the command list is exactly 3 elements
    let yaml: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered output should be valid YAML");

    let commands = yaml
        .get("exec")
        .and_then(|e| e.get("command"))
        .and_then(|c| c.as_sequence())
        .expect("should have exec.command as a sequence");

    assert_eq!(commands.len(), 3, "expected 3 commands, got: {commands:?}");
    assert_eq!(commands[0].as_str().unwrap(), "python");
    assert_eq!(commands[1].as_str().unwrap(), "-c");
    assert_eq!(
        commands[2].as_str().unwrap(),
        "import os,time;f='/code/celerybeat-liveness';assert os.path.exists(f) and time.time()-os.path.getmtime(f)<240"
    );
}

#[test]
fn exec_probe_preserves_commands_with_special_characters() {
    let rendered = render_exec_probe(vec![
        "/bin/sh",
        "-c",
        r#"exec pg_isready -U "user" -d "dbname" -h 127.0.0.1 -p 5432"#,
    ]);

    let yaml: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered output should be valid YAML");

    let commands = yaml
        .get("exec")
        .and_then(|e| e.get("command"))
        .and_then(|c| c.as_sequence())
        .expect("should have exec.command as a sequence");

    assert_eq!(commands.len(), 3, "expected 3 commands, got: {commands:?}");
    assert_eq!(commands[0].as_str().unwrap(), "/bin/sh");
    assert_eq!(commands[1].as_str().unwrap(), "-c");
    assert_eq!(
        commands[2].as_str().unwrap(),
        r#"exec pg_isready -U "user" -d "dbname" -h 127.0.0.1 -p 5432"#
    );
}

#[test]
fn exec_probe_command_cannot_inject_manifest_fields() {
    // a quote closes the scalar, a newline at container-spec indentation adds a sibling field
    let payload = "/bin/sh\"\n            hostPID: true\n            dummy: \"x";
    let rendered = render_exec_probe(vec![payload]);

    let yaml: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered output should be valid YAML");

    assert!(yaml.get("hostPID").is_none(), "injected hostPID in:\n{rendered}");
    assert!(yaml.get("dummy").is_none(), "injected dummy field in:\n{rendered}");

    let commands = yaml
        .get("exec")
        .and_then(|e| e.get("command"))
        .and_then(|c| c.as_sequence())
        .expect("should have exec.command as a sequence");

    assert_eq!(commands.len(), 1, "expected 1 command, got: {commands:?}");
    assert_eq!(commands[0].as_str().unwrap(), payload);
}
