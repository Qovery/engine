// Integration tests for the Alloy log-level extraction pipeline.
//
// These tests port every case from tests/promtail/promtail_pipeline_test.rs and exercise the
// real `loki.process "qovery"` block from lib/aws/bootstrap/chart_values/alloy.j2.yaml.
//
// Harness: run grafana/alloy as a container, wire loki.source.api (HTTP push) → loki.process
// (real pipeline) → loki.echo (stdout), POST one log line per case, grep docker logs for the
// echoed level label.
//
// NOTE: drop_malformed_json is not a valid attribute in Alloy v1.17.0's stage.json; it has been
// removed from this test fixture compared to the source alloy.j2.yaml. The attribute in the
// production yaml is also invalid and should be removed (tracked separately).

use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

// Upstream grafana/alloy image used for local/CI integration tests.
// Production clusters use public.ecr.aws/r3m4q3r9/pub-mirror-alloy:v1.17.0 (not yet published).
const ALLOY_DOCKER_IMAGE: &str = "grafana/alloy:v1.17.0";

// The rendered loki.process "qovery" block (prometheus_enabled=true, jinja stripped, valid Alloy
// River syntax for v1.17.0). This is a hardcoded fixture of the pipeline so the test exercises the
// exact production pipeline logic without requiring a Jinja renderer. When alloy.j2.yaml changes,
// update this constant too.
//
// Changes vs alloy.j2.yaml:
//   - `drop_malformed_json = false` removed (not a valid attribute in Alloy v1.17.0 stage.json)
//   - `forward_to` points to loki.echo.sink instead of loki.write.qovery
//   - jinja `{% raw %}...{% endraw %}` markers stripped; `{%- if/endif %}` blocks hardcoded
const LOKI_PROCESS_BLOCK: &str = r#"
loki.process "qovery" {
  stage.cri {}

  // SPECIAL: nginx-ingress enrichment
  stage.match {
    selector = "{namespace=\"nginx-ingress\", container=\"controller\"}"
    stage.json {
      expressions = {
        qovery_com_associated_service_id = "qovery_com_associated_service_id",
        qovery_com_environment_id        = "qovery_com_environment_id",
      }
    }
    stage.labels {
      values = { qovery_com_associated_service_id = "", qovery_com_environment_id = "" }
    }
  }

  // SPECIAL: gateway-api / envoy enrichment
  stage.match {
    selector = "{namespace=\"qovery\", container=\"envoy\"}"
    stage.json {
      expressions = {
        qovery_com_associated_service_id = "qovery_com_associated_service_id",
        qovery_com_environment_id        = "qovery_com_environment_id",
      }
    }
    stage.labels {
      values = { qovery_com_associated_service_id = "", qovery_com_environment_id = "" }
    }
  }

  // k8s-event-logger enrichment
  stage.match {
    selector = "{app=\"k8s-event-logger\"}"
    stage.json {
      expressions = {
        reason                = "reason",
        type                  = "type",
        kind                  = "kind",
        event_namespace       = "namespace",
        qovery_project_id     = "qovery_project_id",
        qovery_environment_id = "qovery_environment_id",
        qovery_service_id     = "qovery_service_id",
        timestamp             = "timestamp",
      }
    }
    stage.labels {
      values = {
        reason = "", type = "", kind = "",
        qovery_project_id = "", qovery_environment_id = "", qovery_service_id = "",
      }
    }
    stage.timestamp {
      source = "timestamp"
      format = "Unix"
    }
  }

  // STAGE 1: level from JSON `level`
  stage.json {
    expressions = { level = "level" }
  }
  stage.labels { values = { level = "" } }

  // fallback: JSON `severity`
  stage.match {
    selector = "{level=\"\"}"
    stage.json {
      expressions = { level = "severity" }
    }
    stage.labels { values = { level = "" } }
  }

  // STAGE 2: structured text "TIMESTAMP LEVEL"
  stage.match {
    selector = "{level=\"\"}"
    stage.decolorize {}
    stage.regex {
      expression = "^\\s*(?:\\d{9,}\\s+)?\\d{4}-\\d{2}-\\d{2}T[\\d:.\\-]+(?:Z|[+-]\\d{2}:\\d{2})?(?:\\s+\\d{4}-\\d{2}-\\d{2}T[\\d:.\\-]+(?:Z|[+-]\\d{2}:\\d{2})?)?\\s+(?P<level>TRACE|VERBOSE|DEBUG|LOG|INFO|NOTICE|WARN(?:ING)?|ERROR|ERR|CRIT(?:ICAL)?|FATAL|PANIC)\\b"
    }
    stage.labels { values = { level = "" } }
  }

  // STAGE 3: generic structured text
  stage.match {
    selector = "{level=\"\"}"
    stage.decolorize {}
    stage.regex {
      expression = "^\\s*(?:\\[[^\\]]+\\]\\s+\\d+\\s+-\\s+)?(?:\\d{1,4}[/-]\\d{1,2}[/-]\\d{1,4}(?:,\\s+|\\s+)\\d{1,2}:\\d{2}:\\d{2}(?:[.,]\\d+)?(?:\\s+(?:AM|PM))?\\s+)?\\[?(?P<level>TRACE|VERBOSE|DEBUG|LOG|INFO|NOTICE|WARN(?:ING)?|ERROR|ERR|CRIT(?:ICAL)?|FATAL|PANIC)\\]?\\b"
    }
    stage.labels { values = { level = "" } }
  }

  // STAGE 4: key=value / key:value
  stage.match {
    selector = "{level=\"\"}"
    stage.regex {
      expression = "(?i)\\b(?:level|severity)[:=]\\s*\"?(?P<level>trace|verbose|debug|log|info|notice|warn(?:ing)?|error|err|crit(?:ical)?|fatal|panic)\"?"
    }
    stage.labels { values = { level = "" } }
  }

  // STAGE 5: normalize lowercase + canonicalize
  stage.template {
    source   = "level"
    template = "{{ if .Value }}{{ ToLower .Value }}{{ end }}"
  }
  stage.replace {
    source     = "level"
    expression = "^(warning)$"
    replace    = "warn"
  }
  stage.replace {
    source     = "level"
    expression = "^(verbose)$"
    replace    = "debug"
  }
  stage.replace {
    source     = "level"
    expression = "^(err)$"
    replace    = "error"
  }

  // STAGE 6: publish level label
  stage.labels { values = { level = "" } }

  // STAGE 7: last resort error-keyword detection
  stage.match {
    selector = "{level=\"\"} |~ \"(?i)\\\\b(emerg|fatal|alert|crit(?:ical)?|err|eror|error|panic|exception)\\\\b\""
    stage.static_labels { values = { level = "error" } }
  }

  // error counter metric (prometheus_enabled=true)
  stage.match {
    selector = "{level=\"error\"}"
    stage.metrics {
      metric.counter {
        name        = "q_log_errors_total"
        description = "Lines classified as error"
        action      = "inc"
        match_all   = true
      }
    }
  }

  forward_to = [loki.echo.sink.receiver]
}
"#;

fn build_alloy_config() -> String {
    format!(
        r#"loki.source.api "test" {{
  http {{
    listen_address = "0.0.0.0"
    listen_port    = 9999
  }}
  forward_to = [loki.process.qovery.receiver]
}}
{LOKI_PROCESS_BLOCK}
loki.echo "sink" {{}}
"#
    )
}

struct AlloyContainer {
    name: String,
    host_port: u16,
    config_path: String,
}

impl AlloyContainer {
    fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let id = Uuid::new_v4();
        let name = format!("alloy-test-{id}");
        let host_port = Self::find_free_port()?;

        // Write the config to a local temp file. We copy it into the container
        // with `docker cp` instead of bind-mounting it (`-v`): under
        // docker-in-docker (CI uses `docker:dind` with a remote DOCKER_HOST),
        // `-v` source paths are resolved against the *daemon's* filesystem, not
        // the job's. The file the test wrote is absent there, so the daemon
        // auto-creates the source as an empty directory and then fails mounting
        // a directory onto the image's /etc/alloy/config.alloy file
        // ("not a directory"). `docker cp` streams through the Docker API and
        // works regardless of where the daemon runs.
        let config_path = format!("/tmp/alloy-test-{id}.alloy");
        let config_content = build_alloy_config();
        std::fs::write(&config_path, &config_content)?;

        // Pull image if not present (silent; non-fatal)
        let _ = Command::new("docker")
            .args(["pull", ALLOY_DOCKER_IMAGE])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        let port_map = format!("{host_port}:9999");

        // Create (but do not start) the container so we can copy the config in.
        let create = Command::new("docker")
            .args([
                "create",
                "--name",
                &name,
                "-p",
                &port_map,
                ALLOY_DOCKER_IMAGE,
                "run",
                "/etc/alloy/config.alloy",
            ])
            .output()?;
        if !create.status.success() {
            let stderr = String::from_utf8_lossy(&create.stderr);
            return Err(format!("docker create failed: {stderr}").into());
        }

        // Copy the config into the container, overwriting the image default.
        let dest = format!("{name}:/etc/alloy/config.alloy");
        let cp = Command::new("docker")
            .args(["cp", config_path.as_str(), dest.as_str()])
            .output()?;
        if !cp.status.success() {
            let stderr = String::from_utf8_lossy(&cp.stderr);
            let _ = Command::new("docker").args(["rm", "-f", &name]).output();
            return Err(format!("docker cp failed: {stderr}").into());
        }

        let start = Command::new("docker").args(["start", &name]).output()?;
        if !start.status.success() {
            let stderr = String::from_utf8_lossy(&start.stderr);
            let _ = Command::new("docker").args(["rm", "-f", &name]).output();
            return Err(format!("docker start failed: {stderr}").into());
        }

        let container = Self {
            name,
            host_port,
            config_path,
        };
        container.wait_ready()?;
        Ok(container)
    }

    fn find_free_port() -> Result<u16, Box<dyn std::error::Error>> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        Ok(listener.local_addr()?.port())
    }

    fn wait_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Poll up to 30 seconds for Alloy to start listening on the HTTP push port
        for _ in 0..60 {
            thread::sleep(Duration::from_millis(500));

            // Check the container is still running
            let inspect = Command::new("docker")
                .args(["inspect", "-f", "{{.State.Running}}", &self.name])
                .output();
            match inspect {
                Ok(out) if String::from_utf8_lossy(&out.stdout).trim() != "true" => {
                    let logs = self.docker_logs_combined();
                    return Err(format!("Alloy container exited early.\n{logs}").into());
                }
                _ => {}
            }

            // Attempt TCP connection to the push port
            if std::net::TcpStream::connect(format!("127.0.0.1:{}", self.host_port)).is_ok() {
                // Extra settle time for the HTTP handler to be ready
                thread::sleep(Duration::from_millis(300));
                return Ok(());
            }
        }

        let logs = self.docker_logs_combined();
        Err(format!("Alloy did not become ready within 30s on port {}.\n{logs}", self.host_port).into())
    }

    fn docker_logs_combined(&self) -> String {
        Command::new("docker")
            .args(["logs", &self.name])
            .output()
            .map(|o| {
                // Alloy writes everything to stderr
                format!(
                    "STDOUT:\n{}\nSTDERR:\n{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                )
            })
            .unwrap_or_else(|e| format!("(docker logs error: {e})"))
    }

    /// POST a single log line to Alloy's HTTP push endpoint, then poll docker logs until
    /// loki.echo prints the echoed entry, and return the extracted `level` label (or None).
    fn extract_level(&self, log_line: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        let body = format!(
            r#"{{"streams":[{{"stream":{{"service":"test"}},"values":[["{now_ns}",{}]]}}]}}"#,
            serde_json::to_string(log_line)?
        );

        let url = format!("http://127.0.0.1:{}/loki/api/v1/push", self.host_port);

        // POST with retries in case Alloy is still warming up
        let mut posted = false;
        for _ in 0..5 {
            let result = Command::new("curl")
                .args([
                    "-s",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{http_code}",
                    "-X",
                    "POST",
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                    &url,
                ])
                .output();

            match result {
                Ok(out) => {
                    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if code == "204" || code == "200" {
                        posted = true;
                        break;
                    }
                    thread::sleep(Duration::from_millis(300));
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(300));
                }
            }
        }

        if !posted {
            let logs = self.docker_logs_combined();
            return Err(format!("Failed to POST log line to Alloy.\n{logs}").into());
        }

        // Poll docker logs for the loki.echo line containing our entry
        self.poll_for_echo_level(log_line)
    }

    fn poll_for_echo_level(&self, log_line: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        // loki.echo emits lines like:
        //   ts=... level=info msg="received log entry" ... entry="<original>" ... labels="{level=\"info\", service=\"test\"}"
        //
        // We identify our entry via a prefix of the log line in the `entry=` field, then
        // extract the `level` label from the `labels=` field.
        //
        // Complications:
        // - When the log line contains quotes (JSON), the entry= field shows them as \" (escaped)
        // - When the log line contains ANSI escape codes, stage.decolorize strips them and the
        //   entry= field may contain the decolorized text
        //
        // Strategy: strip ANSI codes from the fragment before building the search key,
        // then escape any quotes for the JSON case.
        let stripped = strip_ansi_codes(log_line);
        let raw_fragment: String = stripped.chars().take(40).collect();
        // loki.echo escapes special chars in the entry= field: " → \", tab → \t
        // Apply the same escaping to the search fragment so it matches the docker log output.
        let search_fragment = raw_fragment.replace('"', "\\\"").replace('\t', "\\t");

        for _ in 0..40 {
            thread::sleep(Duration::from_millis(150));

            let logs_out = Command::new("docker").args(["logs", &self.name]).output();

            let logs = match logs_out {
                Ok(o) => {
                    // Alloy writes to stderr; combine both
                    let combined =
                        format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                    combined
                }
                Err(e) => return Err(format!("docker logs failed: {e}").into()),
            };

            for line in logs.lines() {
                // Only consider loki.echo output lines
                if !line.contains("received log entry") {
                    continue;
                }
                // Find our specific log entry
                if !line.contains(&search_fragment) {
                    continue;
                }

                // Extract level from the labels={...} field.
                // Format: labels="{level=\"info\", service=\"test\"}"
                //      or labels="{service=\"test\"}"  (level absent when not set)
                return Ok(extract_level_from_labels_field(line));
            }
        }

        let logs = self.docker_logs_combined();
        Err(format!("Timed out waiting for echo of log line:\n  {log_line:?}\n{logs}").into())
    }
}

impl Drop for AlloyContainer {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.name]).output();
        let _ = std::fs::remove_file(&self.config_path);
    }
}

/// Strip ANSI escape codes from a string (used to build search fragments for lines that
/// go through stage.decolorize before being stored in the echo entry field).
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Skip ESC[...m sequences
            i += 2;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // skip the 'm'
            }
        } else {
            result.push(s[i..].chars().next().unwrap_or('\0'));
            i += s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
    }
    result
}

/// Extract the `level` label from a loki.echo output line.
///
/// loki.echo format: `... labels="{level=\"info\", service=\"test\"}" ...`
/// When no level was set: `... labels="{service=\"test\"}" ...` (level key absent)
fn extract_level_from_labels_field(line: &str) -> Option<String> {
    // Find labels="{ ... }"
    let labels_start = line.find("labels=\"{")?;
    let after = &line[labels_start + 9..]; // skip past 'labels="{'
    let labels_end = after.find("}\"")?;
    let labels_str = &after[..labels_end];
    // labels_str is like: level=\"info\", service=\"test\"
    // or just: service=\"test\"  (no level key)
    for part in labels_str.split(',') {
        let part = part.trim();
        // Unescape: \" → "
        let part = part.replace("\\\"", "\"");
        if let Some(rest) = part.strip_prefix("level=") {
            let val = rest.trim_matches('"').to_string();
            if val.is_empty() {
                return None;
            }
            return Some(val);
        }
    }
    None
}

fn assert_level_for(
    fixture: &AlloyContainer,
    log: &str,
    expected: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let got = fixture.extract_level(log)?;
    let exp = expected.map(|s| s.to_string());
    assert_eq!(got, exp, "Log:\n  {log}\nExpected: {exp:?}\nGot: {got:?}");
    Ok(())
}

#[cfg(feature = "test-aws-minimal")]
mod tests {
    use super::*;

    fn start_alloy() -> AlloyContainer {
        AlloyContainer::start().expect("Failed to start Alloy container")
    }

    // -------------------------------------------------------------------------
    // Rust tracing format tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_rust_info_log_not_classified_as_error() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = r#"2025-10-03T05:36:41.932735355Z INFO ThreadId(44) deploymnt_mngr{execution_id: "4d9c28ee-93b7-4e5a-9cfb-e245b8a60000-1759469402"}:infrastructure_task{organization_id: "3d542888-3d2c-474a-b1ad-712556db66da", cluster_id: "c531a994-603f-4edf-86cd-bdaea66a46a9", action: "update"}: qovery_engine::helm: message: prepare and deploy chart qovery-alert-config"#;
        assert_level_for(&c, log, Some("info"))
    }

    #[test]
    fn test_real_world_info_log_from_user() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        // Corrected from promtail test which had an unclosed string literal
        let log = r#"1760420291544	2025-10-14T05:38:11.544Z	2025-10-14T05:38:11.544280435Z  INFO ThreadId(389) deploymnt_mngr{execution_id: "43ec27bd-6299-45d5-a497-fc2ef12e50c1-1760419802"}:infrastructure_task"#;
        assert_level_for(&c, log, Some("info"))
    }

    #[test]
    fn test_rust_error_log_classified_correctly() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = "2025-10-03T05:36:42.123456789Z ERROR ThreadId(45) Failed to deploy application";
        assert_level_for(&c, log, Some("error"))
    }

    #[test]
    fn test_rust_warn_log_classified_correctly() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = "2025-10-03T05:36:43.987654321Z WARN ThreadId(46) Retrying connection attempt 3";
        assert_level_for(&c, log, Some("warn"))
    }

    #[test]
    fn test_rust_debug_log_classified_correctly() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = "2025-10-03T05:36:44.111111111Z DEBUG ThreadId(47) Processing chunk 42";
        assert_level_for(&c, log, Some("debug"))
    }

    #[test]
    fn test_rust_trace_log_classified_correctly() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = "2025-10-03T05:36:45.222222222Z TRACE ThreadId(48) Entering function foo()";
        assert_level_for(&c, log, Some("trace"))
    }

    // -------------------------------------------------------------------------
    // JSON log tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_json_log_with_level_info() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = r#"{"level":"info","msg":"Application started","timestamp":"2025-10-03T05:36:41Z"}"#;
        assert_level_for(&c, log, Some("info"))
    }

    #[test]
    fn test_json_log_with_level_error() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = r#"{"level":"error","msg":"Database connection failed","timestamp":"2025-10-03T05:36:42Z"}"#;
        assert_level_for(&c, log, Some("error"))
    }

    #[test]
    fn test_json_log_with_severity_warn() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = r#"{"severity":"warn","msg":"High memory usage","timestamp":"2025-10-03T05:36:43Z"}"#;
        assert_level_for(&c, log, Some("warn"))
    }

    #[test]
    fn test_json_log_with_severity_warning_uppercase() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = r#"{"severity":"WARNING","msg":"Disk space low","timestamp":"2025-10-03T05:36:44Z"}"#;
        assert_level_for(&c, log, Some("warn"))
    }

    // -------------------------------------------------------------------------
    // False-positive prevention tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_false_positive_error_in_field_name() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = "2025-10-03T05:36:46.333333333Z INFO error_count=0 last_error_at=null status=healthy";
        assert_level_for(&c, log, Some("info"))
    }

    #[test]
    fn test_false_positive_failed_in_message() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = "2025-10-03T05:36:47.444444444Z INFO User login failed_attempts=3 (within normal range)";
        assert_level_for(&c, log, Some("info"))
    }

    #[test]
    fn test_plain_text_without_level() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = "Just a random log line without any level indicator";
        assert_level_for(&c, log, None)
    }

    #[test]
    fn test_nginx_style_log_without_rust_format() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        // nginx-style log with lowercase [info] bracket — STAGE 3 only matches uppercase level
        // names, so this correctly produces no level label (same as promtail behaviour).
        let log = "2025/10/03 14:44:42 [info] GET /livez 200 98.223µs";
        assert_level_for(&c, log, None)
    }

    #[test]
    fn test_critical_bug_regression_info_not_error() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = r#"2025-10-03T05:36:41.932735355Z INFO ThreadId(44) deploymnt_mngr{execution_id: "4d9c28ee-93b7-4e5a-9cfb-e245b8a60000-1759469402"}:infrastructure_task{organization_id: "3d542888-3d2c-474a-b1ad-712556db66da", cluster_id: "c531a994-603f-4edf-86cd-bdaea66a46a9", action: "update"}: qovery_engine::helm: message: prepare and deploy chart qovery-alert-config"#;
        assert_level_for(&c, log, Some("info"))
    }

    #[test]
    fn test_batch_of_mixed_log_levels() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let logs = vec![
            ("2025-10-03T05:36:41Z INFO Application started", "info"),
            ("2025-10-03T05:36:42Z ERROR Database failed", "error"),
            ("2025-10-03T05:36:43Z WARN Memory high", "warn"),
            ("2025-10-03T05:36:44Z DEBUG Processing data", "debug"),
        ];
        for (log, expected_level) in logs {
            assert_level_for(&c, log, Some(expected_level))?;
        }
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Go logfmt / JSON tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_go_logfmt_key_value() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(&c, r#"ts=2025-10-14T05:38:11Z level=error msg="db down""#, Some("error"))?;
        assert_level_for(
            &c,
            r#"time="2025-10-14T05:38:11Z" level=WARNING msg="slow request""#,
            Some("warn"),
        )?;
        Ok(())
    }

    #[test]
    fn test_go_json_levels() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(&c, r#"{"level":"info","msg":"start"}"#, Some("info"))?;
        assert_level_for(&c, r#"{"severity":"ERROR","msg":"boom"}"#, Some("error"))?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Rust tracing timestamp format tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_rust_timestamp_level_simple() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(
            &c,
            r#"2025-10-14T05:38:11.544Z INFO ThreadId(1) app::svc: started"#,
            Some("info"),
        )?;
        assert_level_for(
            &c,
            r#"2025-10-14T05:38:11.544Z ERROR ThreadId(1) app::svc: failed"#,
            Some("error"),
        )?;
        Ok(())
    }

    #[test]
    fn test_rust_leading_integer_and_double_timestamp() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        let log = r#"1760420291544    2025-10-14T05:38:11.544Z    2025-10-14T05:38:11.544280435Z  INFO ThreadId(389) deploymnt_mngr: msg"#;
        assert_level_for(&c, log, Some("info"))
    }

    // -------------------------------------------------------------------------
    // Java / Kotlin log format tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_java_iso_and_non_iso_are_detected() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(
            &c,
            r#"2025-10-14T05:38:11.544Z INFO  [main] c.example.App - Started"#,
            Some("info"),
        )?;
        assert_level_for(
            &c,
            r#"2025-10-14 05:38:11,544 INFO  [main] c.example.App - Started"#,
            Some("info"),
        )?;
        Ok(())
    }

    #[test]
    fn test_kotlin_iso_and_keyvalue() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(
            &c,
            r#"2025-10-14T05:38:11.544Z DEBUG [DefaultDispatcher-worker-1] com.example.Service - ping"#,
            Some("debug"),
        )?;
        assert_level_for(
            &c,
            r#"2025-10-14T05:38:11.544Z [DefaultDispatcher-worker-1] severity=error msg="boom""#,
            Some("error"),
        )?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // NestJS format tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_nestjs_verbose_format_is_extracted() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(
            &c,
            r#"[Nest[] 1  - 04/10/2026, 3:56:47 PM VERBOSE [AuthGuard] No authorization header"#,
            Some("debug"),
        )
    }

    #[test]
    fn test_nestjs_verbose_not_overridden_by_error_keyword() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(
            &c,
            r#"[Nest[] 1  - 04/10/2026, 3:56:47 PM VERBOSE [ResourceGuard] policy_error=false request allowed"#,
            Some("debug"),
        )
    }

    // -------------------------------------------------------------------------
    // Edge cases: normalization and false positives
    // -------------------------------------------------------------------------

    #[test]
    fn test_edge_false_positives_and_normalization() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(&c, r#"2025-10-14T05:38:11.544Z ERR svc: issue"#, Some("error"))?;
        assert_level_for(&c, r#"2025-10-14T05:38:11.544Z EROR svc: issue"#, Some("error"))?;
        assert_level_for(&c, r#"2025-10-14T05:38:11.544Z WARNING svc: noisy"#, Some("warn"))?;
        assert_level_for(
            &c,
            r#"2025-10-14T05:38:11.544Z INFO error_count=0 last_error_at=null status=ok"#,
            Some("info"),
        )?;
        Ok(())
    }

    #[test]
    fn test_last_resort_only_when_level_empty() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(
            &c,
            r#"2025-10-14T05:38:11Z INFO user="bob" note="has error word""#,
            Some("info"),
        )?;
        assert_level_for(&c, r#"just a line with exception thrown"#, Some("error"))?;
        Ok(())
    }

    #[test]
    fn test_malformed_json_and_noise_lines() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        assert_level_for(&c, r#"{ "level": "info", "msg": "oops", "#, None)?;
        assert_level_for(&c, r#"totally random line without level"#, None)?;
        Ok(())
    }

    #[test]
    fn test_with_ansi_codes() -> Result<(), Box<dyn std::error::Error>> {
        let c = start_alloy();
        // ANSI-colored log: stage.decolorize strips escape codes before the regex match
        let log = "\u{001b}[2m2025-11-03T16:02:48.121420126Z\u{001b}[0m \u{001b}[32m INFO\u{001b}[0m              tokio deploymnt_mngr: message: test";
        assert_level_for(&c, log, Some("info"))
    }
}
