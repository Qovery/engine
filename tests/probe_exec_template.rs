use serde_json::json;
use std::path::Path;
use std::process::Command;

/// Exercise the shipped chart: values are serialized as data and Helm evaluates the template once.
fn assert_exec_commands(commands: &[&str]) {
    for is_cron in [false, true] {
        let values = json!({
            "namespace": "test", "environment_short_id": "env", "environment_long_id": "env-long",
            "project_long_id": "project", "deployment_id": "deployment",
            "cluster": {"is_karpenter_enabled": true}, "registry": null,
            "labels_group": {"common": {}},
            "annotations_group": {"job": {}, "cronjob": {}, "pods": {}, "secrets": {}},
            "mounted_files": [], "external_secrets": [], "environment_variables": [],
            "service": {
                "name": "test-job", "long_id": "service", "image_tag_label": "latest",
                "version": "v1", "image_full": "example.com/job:latest",
                "cronjob_schedule": if is_cron { Some("*/5 * * * *") } else { None },
                "cronjob_timezone": "UTC", "max_nb_restart": 1, "max_duration_in_sec": 120,
                "command_args": [], "cpu_request_in_milli": "100m", "ram_request_in_mib": "128Mi",
                "ram_limit_in_mib": "256Mi",
                "liveness_probe": {
                    "type": {"exec": {"commands": commands}}, "initial_delay_seconds": 1,
                    "period_seconds": 2, "timeout_seconds": 3, "success_threshold": 1, "failure_threshold": 4
                },
                "advanced_settings": {
                    "security_service_account_name": "", "security_automount_service_account_token": false,
                    "security_read_only_root_filesystem": false, "deployment_termination_grace_period_seconds": 30,
                    "cronjob_concurrency_policy": "Forbid", "cronjob_failed_jobs_history_limit": 1,
                    "cronjob_success_jobs_history_limit": 1
                }
            }
        });
        let temp = tempfile::tempdir().unwrap();
        let values_path = temp.path().join("values.json");
        std::fs::write(&values_path, serde_json::to_vec(&values).unwrap()).unwrap();
        let output = Command::new("helm")
            .args(["template", "test-job"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("lib/common/charts/q-job"))
            .arg("--values")
            .arg(values_path)
            .arg("--show-only")
            .arg(if is_cron {
                "templates/cronjob.yaml"
            } else {
                "templates/job.yaml"
            })
            .output()
            .expect("helm must be installed to test the actual job chart");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let manifest: serde_yaml::Value = serde_yaml::from_slice(&output.stdout).unwrap();
        let pod = if is_cron {
            &manifest["spec"]["jobTemplate"]["spec"]["template"]["spec"]
        } else {
            &manifest["spec"]["template"]["spec"]
        };
        let container = &pod["containers"][0];
        let probe = &container["livenessProbe"];
        let actual: Vec<_> = probe["exec"]["command"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(actual, commands);
        for node in [&manifest, pod, container, probe, &probe["exec"]] {
            assert!(node.get("hostPID").is_none());
            assert!(node.get("dummy").is_none());
        }
    }
}

#[test]
fn exec_probe_preserves_commands_with_commas() {
    assert_exec_commands(&[
        "python",
        "-c",
        "import os,time;f='/code/celerybeat-liveness';assert os.path.exists(f) and time.time()-os.path.getmtime(f)<240",
    ]);
}

#[test]
fn exec_probe_preserves_commands_with_special_characters() {
    assert_exec_commands(&[
        "/bin/sh",
        "-c",
        r#"exec pg_isready -U "user" -d "dbname" -h 127.0.0.1 -p 5432"#,
    ]);
}

#[test]
fn exec_probe_command_cannot_inject_manifest_fields() {
    assert_exec_commands(&["/bin/sh\"\n            hostPID: true\n            dummy: \"x"]);
}

#[test]
fn exec_probe_preserves_literal_helm_and_yaml_values() {
    assert_exec_commands(&[
        "{{ fail \"must not execute\" }}",
        "{{ lookup \"v1\" \"Secret\" \"\" \"\" }}",
        "yes",
        "off",
        "1_000",
    ]);
}
