use once_cell::sync::Lazy;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const PROMTAIL_DOCKER_IMAGE: &str = "grafana/promtail:3.5.1";
const CONTAINER_NAME: &str = "promtail-test-runner";

static PROMTAIL_CONTAINER: Lazy<Mutex<PromtailContainer>> = Lazy::new(|| Mutex::new(PromtailContainer::new()));

struct PromtailContainer {}

impl PromtailContainer {
    fn new() -> Self {
        Self {}
    }

    fn ensure_running(&self) -> Result<(), Box<dyn std::error::Error>> {
        let inspect = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", CONTAINER_NAME])
            .output();

        let is_running = match inspect {
            Ok(output) => String::from_utf8_lossy(&output.stdout).trim() == "true",
            Err(_) => false,
        };

        if is_running {
            return Ok(());
        }

        let _ = Command::new("docker").args(["rm", "-f", CONTAINER_NAME]).output();

        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                CONTAINER_NAME,
                "--entrypoint",
                "/bin/sh",
                PROMTAIL_DOCKER_IMAGE,
                "-c",
                "sleep infinity",
            ])
            .output()?;

        if !output.status.success() {
            return Err(format!("Failed to start container: {}", String::from_utf8_lossy(&output.stderr)).into());
        }

        thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    fn exec_promtail(&self, config_path: &str, log: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.ensure_running()?;

        let config_name = format!("config-{}.yml", rand::random::<u32>());

        let copy_result = Command::new("docker")
            .args(["cp", config_path, &format!("{CONTAINER_NAME}:/tmp/{config_name}")])
            .output()?;

        if !copy_result.status.success() {
            return Err(format!("Failed to copy config: {}", String::from_utf8_lossy(&copy_result.stderr)).into());
        }

        let mut child = Command::new("docker")
            .args([
                "exec",
                "-i",
                CONTAINER_NAME,
                "/usr/bin/promtail",
                &format!("--config.file=/tmp/{config_name}"),
                "--dry-run",
                "--stdin",
                "--client.external-labels=namespace=qovery-engine,container=engine,stream=stdout",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(log.as_bytes())?;
            stdin.write_all(b"\n")?;
            drop(stdin);
        }

        let output = child.wait_with_output()?;

        if !output.status.success() {
            return Err(format!(
                "Promtail failed:\nSTDOUT: {}\nSTDERR: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl Drop for PromtailContainer {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", CONTAINER_NAME]).output();
    }
}

fn get_promtail_template_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("lib/aws/bootstrap/chart_values/promtail.j2.yaml")
}

fn load_promtail_template() -> Result<String, Box<dyn std::error::Error>> {
    let template_path = get_promtail_template_path();
    if !template_path.exists() {
        return Err(format!("Template not found: {template_path:?}").into());
    }
    fs::read_to_string(&template_path).map_err(Into::into)
}

fn extract_pipeline_stages(template: &str) -> Result<String, Box<dyn std::error::Error>> {
    let pipeline_start = template.find("pipelineStages:").ok_or("pipelineStages not found")?;

    let after_pipeline = &template[pipeline_start..];
    let pipeline_end = after_pipeline
        .find("\n    extraRelabelConfigs:")
        .unwrap_or(after_pipeline.len());

    let pipeline_section = &after_pipeline[..pipeline_end];

    let cleaned = pipeline_section
        .replace("{%- if prometheus_enabled == true %}", "")
        .replace("{%- endif %}", "")
        .replace("{% raw %}", "")
        .replace("{% endraw %}", "")
        .replace("{ }", "{}");

    Ok(cleaned.replace("pipelineStages:", "pipeline_stages:"))
}

fn create_full_config_from_template(positions_file: &str) -> Result<String, Box<dyn std::error::Error>> {
    let template = load_promtail_template()?;
    let pipeline_stages = extract_pipeline_stages(&template)?;

    Ok(format!(
        r#"server:
  http_listen_port: 9080
  grpc_listen_port: 0

positions:
  filename: {positions_file}

clients:
  - url: http://localhost:3100/loki/api/v1/push

scrape_configs:
  - job_name: test
    static_configs:
      - targets:
          - localhost
        labels:
          job: test
          namespace: qovery-engine
          container: engine
          stream: stdout
    {pipeline_stages}
"#,
    ))
}

struct PromtailTestFixture {
    _temp_dir: TempDir,
    config_path: PathBuf,
}

impl PromtailTestFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let positions_file = temp_dir.path().join("positions.yaml");

        let config = create_full_config_from_template(&positions_file.to_string_lossy())?;

        let config_path = temp_dir.path().join("promtail-config.yml");
        fs::write(&config_path, &config)?;

        Ok(Self {
            _temp_dir: temp_dir,
            config_path,
        })
    }

    fn extract_level(&self, log: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let container = PROMTAIL_CONTAINER.lock().unwrap();
        let output = container.exec_promtail(&self.config_path.to_string_lossy(), log)?;

        for line in output.lines() {
            if let Some(start) = line.find('{') {
                if let Some(end) = line.find('}') {
                    let labels_str = &line[start + 1..end];

                    for label_pair in labels_str.split(',') {
                        let parts: Vec<&str> = label_pair.trim().splitn(2, '=').collect();
                        if parts.len() == 2 {
                            let key = parts[0].trim();
                            let value = parts[1].trim().trim_matches('"');

                            if key == "level" {
                                return Ok(Some(value.to_string()));
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

fn ensure_docker_image() -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("docker")
        .args(["pull", PROMTAIL_DOCKER_IMAGE])
        .stdout(Stdio::null())
        .status()?;

    if !status.success() {
        return Err("Failed to pull image".into());
    }
    Ok(())
}

#[cfg(feature = "test-aws-minimal")]
mod tests {
    use super::*;

    #[test]
    fn test_rust_info_log_not_classified_as_error() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = r#"2025-10-03T05:36:41.932735355Z INFO ThreadId(44) deploymnt_mngr{execution_id: "4d9c28ee-93b7-4e5a-9cfb-e245b8a60000-1759469402"}:infrastructure_task{organization_id: "3d542888-3d2c-474a-b1ad-712556db66da", cluster_id: "c531a994-603f-4edf-86cd-bdaea66a46a9", action: "update"}: qovery_engine::helm: message: prepare and deploy chart qovery-alert-config"#;

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("info".to_string()),
            "Rust INFO log should be classified as 'info', not '{level:?}'",
        );

        Ok(())
    }

    #[test]
    fn test_rust_error_log_classified_correctly() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = "2025-10-03T05:36:42.123456789Z ERROR ThreadId(45) Failed to deploy application";

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("error".to_string()),
            "Rust ERROR log should be classified as 'error'"
        );

        Ok(())
    }

    #[test]
    fn test_rust_warn_log_classified_correctly() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = "2025-10-03T05:36:43.987654321Z WARN ThreadId(46) Retrying connection attempt 3";

        let level = fixture.extract_level(log)?;

        assert_eq!(level, Some("warn".to_string()), "Rust WARN log should be classified as 'warn'");

        Ok(())
    }

    #[test]
    fn test_rust_debug_log_classified_correctly() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = "2025-10-03T05:36:44.111111111Z DEBUG ThreadId(47) Processing chunk 42";

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("debug".to_string()),
            "Rust DEBUG log should be classified as 'debug'"
        );

        Ok(())
    }

    #[test]
    fn test_rust_trace_log_classified_correctly() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = "2025-10-03T05:36:45.222222222Z TRACE ThreadId(48) Entering function foo()";

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("trace".to_string()),
            "Rust TRACE log should be classified as 'trace'"
        );

        Ok(())
    }

    #[test]
    fn test_json_log_with_level_info() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = r#"{"level":"info","msg":"Application started","timestamp":"2025-10-03T05:36:41Z"}"#;

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("info".to_string()),
            "JSON log with level=info should be classified as 'info'"
        );

        Ok(())
    }

    #[test]
    fn test_json_log_with_level_error() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = r#"{"level":"error","msg":"Database connection failed","timestamp":"2025-10-03T05:36:42Z"}"#;

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("error".to_string()),
            "JSON log with level=error should be classified as 'error'"
        );

        Ok(())
    }

    #[test]
    fn test_json_log_with_severity_warn() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = r#"{"severity":"warn","msg":"High memory usage","timestamp":"2025-10-03T05:36:43Z"}"#;

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("warn".to_string()),
            "JSON log with severity=warn should be mapped to level=warn"
        );

        Ok(())
    }

    #[test]
    fn test_json_log_with_severity_warning_uppercase() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = r#"{"severity":"WARNING","msg":"Disk space low","timestamp":"2025-10-03T05:36:44Z"}"#;

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("warn".to_string()),
            "JSON log with severity=WARNING (uppercase) should be normalized to 'warn'"
        );

        Ok(())
    }

    #[test]
    fn test_false_positive_error_in_field_name() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = "2025-10-03T05:36:46.333333333Z INFO error_count=0 last_error_at=null status=healthy";

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("info".to_string()),
            "INFO log with 'error' in field name should NOT be classified as error"
        );

        Ok(())
    }

    #[test]
    fn test_false_positive_failed_in_message() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = "2025-10-03T05:36:47.444444444Z INFO User login failed_attempts=3 (within normal range)";

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("info".to_string()),
            "INFO log with 'failed' in message should NOT be classified as error"
        );

        Ok(())
    }

    #[test]
    fn test_plain_text_without_level() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = "Just a random log line without any level indicator";

        let level = fixture.extract_level(log)?;

        assert_eq!(level, None, "Plain text without level should have no level label");

        Ok(())
    }

    #[test]
    fn test_nginx_style_log_without_rust_format() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = "2025/10/03 14:44:42 [info] GET /livez 200 98.223µs";

        let level = fixture.extract_level(log)?;

        assert_eq!(level, None, "Nginx-style log should not be captured by Rust-specific rules");

        Ok(())
    }

    #[test]
    fn test_critical_bug_regression_info_not_error() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let log = r#"2025-10-03T05:36:41.932735355Z INFO ThreadId(44) deploymnt_mngr{execution_id: "4d9c28ee-93b7-4e5a-9cfb-e245b8a60000-1759469402"}:infrastructure_task{organization_id: "3d542888-3d2c-474a-b1ad-712556db66da", cluster_id: "c531a994-603f-4edf-86cd-bdaea66a46a9", action: "update"}: qovery_engine::helm: message: prepare and deploy chart qovery-alert-config"#;

        let level = fixture.extract_level(log)?;

        assert_eq!(
            level,
            Some("info".to_string()),
            "CRITICAL REGRESSION: The bug is back! This INFO log is being classified as '{level:?}' instead of 'info'",
        );

        Ok(())
    }

    #[test]
    fn test_batch_of_mixed_log_levels() -> Result<(), Box<dyn std::error::Error>> {
        ensure_docker_image()?;
        let fixture = PromtailTestFixture::new()?;

        let logs = vec![
            ("2025-10-03T05:36:41Z INFO Application started", "info"),
            ("2025-10-03T05:36:42Z ERROR Database failed", "error"),
            ("2025-10-03T05:36:43Z WARN Memory high", "warn"),
            ("2025-10-03T05:36:44Z DEBUG Processing data", "debug"),
        ];

        for (log, expected_level) in logs {
            let level = fixture.extract_level(log)?;
            assert_eq!(
                level,
                Some(expected_level.to_string()),
                "Log '{log}' should be classified as '{expected_level}'",
            );
        }

        Ok(())
    }
}
