use serde_json::json;
use tera::{Context, Tera};

/// Minimal template snippet that mirrors the exec probe rendering in
/// `q-container/templates/deployment.j2.yaml` (and statefulset, job, cronjob).
///
/// The template iterates over each command element and emits it as a YAML
/// literal block scalar (`|-`).  This guarantees that strings containing
/// commas, semicolons, quotes, or other special characters are never split
/// or mangled by the YAML parser.
const PROBE_EXEC_TEMPLATE: &str = r#"
{%- if service.liveness_probe.type.exec %}
            exec:
              command:
              {%- for cmd in service.liveness_probe.type.exec.commands %}
              - |-
                {{ cmd }}
              {%- endfor %}
{%- endif %}
"#;

fn render_exec_probe(commands: Vec<&str>) -> String {
    let mut tera = Tera::default();
    tera.add_raw_template("template", PROBE_EXEC_TEMPLATE)
        .expect("exec probe template should parse");

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

    tera.render("template", &context)
        .expect("exec probe template should render")
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
