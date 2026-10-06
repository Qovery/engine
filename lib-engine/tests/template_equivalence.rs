//! Compares native Helm manifests with frozen Tera fixtures from before the migration.
//!
//! Both versions receive the same context. Parsed Kubernetes objects must remain equivalent,
//! apart from runtime timestamps, formatting-dependent checksums, and base64 whitespace.
//! Per-case overrides exercise conditional resources and probe variants.

// the shared context is one large object literal
#![recursion_limit = "512"]

use qovery_engine::environment::action::deploy_helm::{HelmChartValues, HelmDeployment};
use qovery_engine::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
use qovery_engine::helm::ChartInfo;
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::tera_utils::render_one_off;
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::path::Path;
use std::process::Command;
use tera::Context;
use uuid::Uuid;

fn read(path: String) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn fixture(family: &str, name: &str) -> String {
    read(format!(
        "{}/tests/fixtures/pre_escaping/{family}/{name}.j2.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// Production chart preparation serializes context into values, then Helm renders the templates.
fn render_current(family: &str, name: &str, context: &Context) -> Result<String, tera::Error> {
    let chart = tempfile::tempdir().expect("temporary chart");
    let event_details = EventDetails::new(
        None,
        QoveryIdentifier::new_random(),
        QoveryIdentifier::new_random(),
        Uuid::new_v4().to_string(),
        Stage::Infrastructure(InfrastructureStep::RetrieveClusterConfig),
        Transmitter::TaskManager(Uuid::new_v4(), "engine".to_string()),
    );
    HelmDeployment::new(
        event_details,
        context.clone(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("lib/common/charts/{family}")),
        HelmChartValues::SerializedContext,
        ChartInfo {
            path: chart.path().to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .prepare_helm_chart()
    .expect("prepare native Helm chart");
    let output = Command::new("helm")
        .args([
            "template",
            "test",
            chart.path().to_str().expect("chart path"),
            "--show-only",
        ])
        .arg(format!("templates/{name}.yaml"))
        .output()
        .expect("helm must be installed to test chart rendering");
    if !output.status.success() {
        return Err(tera::Error::msg(String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8(output.stdout).expect("Helm output should be UTF-8"))
}

/// Normalize runtime-only Helm actions in the legacy fixtures before comparing parsed objects.
fn parse_as_kubernetes_would(rendered: &str) -> Vec<Json> {
    let helm_action = regex::Regex::new(r"\{\{.*?\}\}").expect("valid regex");
    let helm_rendered = helm_action.replace_all(rendered, "helm-rendered");

    serde_yaml::Deserializer::from_str(&helm_rendered)
        .map(|doc| serde_yaml::Value::deserialize(doc).expect("rendered manifest must parse as YAML"))
        .filter(|doc| !doc.is_null())
        .map(|doc| {
            let mut doc = serde_json::to_value(doc).expect("YAML maps to JSON");
            // Ignore runtime timestamps and the text-format-dependent secret checksums.
            for pointer in [
                "/metadata/annotations/releaseTime",
                "/spec/template/metadata/annotations/checksum~1config",
                "/spec/template/metadata/annotations/checksum~1config-mount-files",
            ] {
                if let Some(value) = doc.pointer_mut(pointer) {
                    *value = json!("helm-rendered");
                }
            }
            // Kubernetes base64 decoding ignores CRLF in legacy block-scalar secret data.
            if doc["kind"] == "Secret"
                && let Some(data) = doc.get_mut("data").and_then(Json::as_object_mut)
            {
                for value in data.values_mut() {
                    if let Some(encoded) = value.as_str() {
                        *value = json!(encoded.replace(['\r', '\n'], ""));
                    }
                }
            }
            doc
        })
        .collect()
}

fn assert_family_equivalent(family: &str, cases: Vec<(&str, Context)>) {
    let mut failures: Vec<String> = vec![];

    for (name, context) in cases {
        let (before, after) = match (
            render_one_off(&fixture(family, name), &context),
            render_current(family, name, &context),
        ) {
            (Ok(before), Ok(after)) => (before, after),
            (Err(e), _) => {
                failures.push(format!("{family}/{name}: fixture failed to render: {e:?}"));
                continue;
            }
            (_, Err(e)) => {
                failures.push(format!("{family}/{name}: current template failed to render: {e:?}"));
                continue;
            }
        };

        let (mut before, mut after) = (parse_as_kubernetes_would(&before), parse_as_kubernetes_would(&after));
        // Helm orders documents by resource kind/name; compare resources independently of that ordering.
        for documents in [&mut before, &mut after] {
            documents.sort_by(|a, b| {
                a["kind"]
                    .as_str()
                    .cmp(&b["kind"].as_str())
                    .then_with(|| a["metadata"]["name"].as_str().cmp(&b["metadata"]["name"].as_str()))
            });
        }
        if before.is_empty() {
            failures.push(format!("{family}/{name}: fixture rendered no document, so this proves nothing"));
        } else if before != after {
            failures.push(format!(
                "{family}/{name}: the rendered manifest differs from the fixture.\n\
                 Review the behavioral difference against the frozen pre-migration fixture.\n\
                 before: {before:?}\nafter:  {after:?}"
            ));
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

fn set(target: &mut Json, path: &str, value: Json) {
    let mut cursor = target;
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            cursor[segment] = value;
            return;
        }
        cursor = cursor
            .get_mut(segment)
            .unwrap_or_else(|| panic!("no such path segment: {segment}"));
    }
}

/// Ordinary values for every field the escaping pass touches. A field left empty here is a
/// transformation these tests cannot see.
fn base() -> Json {
    let annotations = json!({ "example.com/owner": "sre" });
    json!({
        "organization_long_id": "00000000-0000-0000-0000-0000000000a1",
        "project_long_id": "00000000-0000-0000-0000-0000000000a2",
        "project_id": "prj12345",
        "environment_long_id": "00000000-0000-0000-0000-0000000000a3",
        "environment_short_id": "env12345",
        "environment_id": "env12345",
        "service_id": "svc12345",
        "service_type": "container",
        "associated_service_long_id": "00000000-0000-0000-0000-0000000000a4",
        "associated_service_type": "application",
        "namespace": "test-namespace",
        "namespace_key": "test-namespace",
        "deployment_id": "deploy12345",
        "id": "z1234567",
        "long_id": "00000000-0000-0000-0000-0000000000a5",
        "sanitized_name": "router-name",
        "resource_expiration_in_seconds": 3600,
        "cluster": { "long_id": "00000000-0000-0000-0000-0000000000a6", "name": "test-cluster",
                     "region": "eu-west-3", "zone": "eu-west-3a", "is_karpenter_enabled": false },
        "labels_group": { "common": { "team": "platform" }, "propagated_to_cloud_provider": { "team": "platform" } },
        "annotations_group": {
            "stateful_set": annotations, "deployment": annotations, "service": annotations,
            "pods": annotations, "secrets": annotations, "hpa": annotations, "ingress": annotations,
            "gateway_api_routes": annotations, "job": annotations, "cronjob": annotations,
        },
        "environment_variables": [{ "key": "DATABASE_URL", "value": "cG9zdGdyZXM6Ly9sb2NhbGhvc3Q=", "is_secret": false }],
        "user_environment_variables": [{ "key": "USER_FLAG", "value": "dHJ1ZQ==", "is_secret": false }],
        // Claude context; Bedrock overrides this below.
        "bedrock_credentials": [],
        "external_secrets": [{
            "secret_name": "svc-external", "external_secret_kube_name": "svc-es", "store_name": "aws-store",
            "entries": [{ "env_var_key": "API_KEY", "remote_key": "prod/api/key", "mount_path": "",
                          "mount_path_relative": "", "volume_name": "es-vol" }],
        }],
        "mounted_files": [{ "long_id": "00000000-0000-0000-0000-0000000000a7", "kube_name": "mf-1",
                            "mount_path": "/etc/app/config.yaml", "file_content_b64": "Y29uZmln" }],
        "loadbalancer_l4_annotations": [["service.beta.kubernetes.io/aws-load-balancer-type", "nlb"]],
        "registry": { "secret_name": "registry-secret", "docker_json_config": "eyJhdXRocyI6e319" },
        "backend_config": { "secret_name": "backend-config", "configs": ["bucket = \"tfstate\""] },
        "basic_auth_htaccess": "dXNlcjokYXByMSQ=",
        "certificate_alternative_names": [{ "domain": "app.example.com" }],
        "has_wildcard_domain": true,
        "http_hosts_has_regex_path": false,
        "grpc_hosts_has_regex_path": false,
        "k8s_deploy_api_gateway": true,
        "k8s_use_api_gateway": false,
        "k8s_deploy_listenerset": true,
        "k8s_deploy_client_traffic_policy_listenerset": true,
        "k8s_remove_nginx": false,
        "nginx_ingress_controller_configuration_snippet": "",
        "nginx_ingress_controller_server_snippet": "",
        "resolved_gateway_api_retry_num_retries": null,
        "resolved_gateway_api_retry_per_try_timeout_seconds": null,
        "resolved_gateway_api_retry_retry_on_triggers": [],
        "resolved_gateway_api_retry_http_status_codes": [],
        "cluster_envoy_gateway_api_http_request_timeout_seconds": null,
        "cluster_envoy_gateway_api_http_connection_idle_timeout_seconds": null,
        "cluster_envoy_gateway_api_http_max_stream_duration_seconds": null,
        "cluster_envoy_client_ip_detection_x_forwarded_for_number_trusted_hops": null,
        "cluster_envoy_client_ip_detection_x_forwarded_for_trusted_cidrs": [],
        "cluster_envoy_client_validation_ca_certificates": [],
        "cluster_envoy_gateway_api_http_stream_idle_timeout_seconds": null,
        "cluster_envoy_gateway_api_path_disable_merge_slashes": false,
        "cluster_envoy_gateway_api_path_escaped_slashes_action": "UnescapeAndRedirect",
        "cluster_envoy_proxy_protocol_enabled": false,
        "cluster_envoy_enable_compression": false,
        "cluster_envoy_custom_http_errors_default": null,
        "cluster_envoy_gateway_api_retry_num_retries": null,
        "cluster_envoy_gateway_api_retry_retry_on": "",
        "cluster_envoy_gateway_api_retry_http_status_codes": "",
        "cluster_envoy_gateway_api_retry_per_try_timeout_seconds": null,
        "service": {
            "short_id": "svc12345",
            "long_id": "00000000-0000-0000-0000-0000000000a8",
            "type": "container",
            "bedrock": null,
            "name": "test-container",
            "kube_name": "test-container",
            "user_unsafe_name": "Test Container",
            "image_full": "registry.example.com/test-image:latest",
            "image_tag": "latest",
            "image_tag_label": "latest",
            "version": "test-image:latest",
            "command_args": ["sh", "-c", "echo ready"],
            "entrypoint": "/bin/sh -c 'exec app'",
            "cpu_request_in_milli": "250m",
            "cpu_limit_in_milli": "250m",
            "ram_request_in_mib": "256Mi",
            "ram_limit_in_mib": "256Mi",
            "gpu_request": null,
            "gpu_limit": null,
            "ephemeral_storage_in_gib": "5Gi",
            "min_instances": 1,
            "max_instances": 1,
            "public_domain": "test.example.com",
            "ports": [],
            "ports_layer4_public": [],
            "default_port": null,
            "storages": [],
            // every probe kind is present as a key so the template's `{% if type.x %}` guards see
            // a defined value; readiness exercises the http path, liveness the exec commands, and
            // the tcp/grpc paths get their own cases below
            "readiness_probe": probe(json!({ "tcp": null, "http": { "path": "/health", "scheme": "HTTP" },
                                             "exec": null, "grpc": null })),
            "liveness_probe": probe(json!({ "tcp": null, "http": null,
                                            "exec": { "commands": ["sh", "-c", "test -f /tmp/alive"] },
                                            "grpc": null })),
            "legacy_deployment_matchlabels": false,
            "legacy_volumeclaim_template": false,
            "legacy_deployment_from_scaleway": false,
            "tolerations": { "nodepool/app": "NoSchedule" },
            "deployment_affinity_node_preferred": { "karpenter.sh/capacity-type": "spot" },
            "autoscaling": null,
            "with_rbac": true,
            "cronjob_schedule": "*/5 * * * *",
            "cronjob_timezone": "UTC",
            "job_max_duration_in_sec": 300,
            "max_duration_in_sec": 300,
            "max_nb_restart": 1,
            "persistence_size_in_gib": "10Gi",
            "persistence_storage_type": "gp3",
            "inputs_json_b64": "e30=",
            "prompt_b64": "cHJvbXB0",
            "advanced_settings": {
                "deployment_affinity_node_required": { "karpenter.sh/nodepool": "app" },
                "deployment_antiaffinity_pod": "Preferred",
                "deployment_topology_spread_zone": "Disabled",
                "deployment_termination_grace_period_seconds": 60,
                "deployment_update_strategy_type": "RollingUpdate",
                "deployment_update_strategy_rolling_update_max_surge_percent": 25,
                "deployment_update_strategy_rolling_update_max_unavailable_percent": 25,
                "deployment_lifecycle_post_start_exec_command": ["sh", "-c", "echo start"],
                "deployment_lifecycle_pre_stop_exec_command": ["sh", "-c", "echo stop"],
                "delete_ttl_seconds_after_finished": null,
                "job_delete_ttl_seconds_after_finished": null,
                "cronjob_concurrency_policy": "Forbid",
                "cronjob_failed_jobs_history_limit": 1,
                "cronjob_success_jobs_history_limit": 1,
                "hpa_cpu_average_utilization_percent": 60,
                "hpa_memory_average_utilization_percent": null,
                "network_dns_ndots": 5,
                "security_automount_service_account_token": false,
                "security_read_only_root_filesystem": false,
                "security_service_account_name": "app-sa",
            },
        },
        "advanced_settings": {
            "network_ingress_add_headers": { "X-Frame-Options": "DENY" },
            "network_ingress_proxy_set_headers": { "X-Forwarded-Host": "app.example.com" },
            "network_ingress_cors_enable": true,
            "network_ingress_cors_allow_origin": "https://app.example.com",
            "network_ingress_cors_allow_methods": "GET,POST",
            "network_ingress_cors_allow_headers": "authorization,content-type",
            "network_ingress_whitelist_source_range": "10.0.0.0/8",
            "network_ingress_denylist_source_range": "192.168.0.0/16",
            "network_ingress_proxy_buffering": "on",
            "network_ingress_proxy_request_buffering": "on",
            "network_ingress_basic_auth_env_var": "",
            "network_ingress_sticky_session_enable": false,
            "network_ingress_force_ssl_redirect": true,
            "network_ingress_proxy_body_size_mb": 100,
            "network_ingress_proxy_buffer_size_kb": 4,
            "network_ingress_proxy_connect_timeout_seconds": 60,
            "network_ingress_proxy_send_timeout_seconds": 60,
            "network_ingress_proxy_read_timeout_seconds": 60,
            "network_ingress_send_timeout_seconds": 60,
            "network_ingress_keepalive_time_seconds": 3600,
            "network_ingress_keepalive_timeout_seconds": 60,
            "network_ingress_grpc_read_timeout_seconds": 60,
            "network_ingress_grpc_send_timeout_seconds": 60,
            "network_ingress_nginx_limit_rpm": null,
            "network_ingress_nginx_limit_rps": null,
            "network_ingress_nginx_limit_burst_multiplier": null,
            "network_ingress_nginx_limit_connections": null,
            "network_ingress_nginx_custom_http_errors": "404,503",
            "network_gateway_api_add_headers": { "X-Frame-Options": "DENY" },
            "network_gateway_api_proxy_set_headers": { "X-Forwarded-Host": "app.example.com" },
            "network_gateway_api_enable_cors": true,
            "network_gateway_api_cors_allow_origin": "https://app.example.com",
            "network_gateway_api_cors_allow_methods": "GET,POST",
            "network_gateway_api_cors_allow_headers": "authorization,content-type",
            "network_gateway_api_whitelist_source_range": "10.0.0.0/8",
            "network_gateway_api_denylist_source_range": "192.168.0.0/16",
            "network_gateway_api_basic_auth_env_var": "",
            "network_gateway_api_force_ssl_redirect": true,
            "network_gateway_api_sticky_session_enable": false,
            "network_gateway_api_sticky_session_type": "Cookie",
            "network_gateway_api_route_limit_rpm": null,
            "network_gateway_api_route_limit_rps": null,
            "network_gateway_api_route_limit_source_cidrs": "10.0.0.0/8",
            "network_gateway_api_route_limit_headers": "X-Api-Key",
            "network_gateway_api_custom_http_errors": "404,503",
            "network_gateway_api_circuit_breaker_max_connections": null,
            "network_gateway_api_circuit_breaker_max_pending_requests": null,
            "network_gateway_api_circuit_breaker_max_parallel_requests": null,
            "network_gateway_api_http_request_timeout_seconds": null,
            "network_gateway_api_http_connection_idle_timeout_seconds": null,
            "network_gateway_api_http_max_stream_duration_seconds": null,
            "network_gateway_api_tcp_keepalive_idle_time_seconds": null,
            "network_gateway_api_tcp_keepalive_interval_seconds": null,
        },
    })
}

fn probe(kind: Json) -> Json {
    json!({ "port": 8080, "initial_delay_seconds": 10, "period_seconds": 10, "timeout_seconds": 5,
            "success_threshold": 1, "failure_threshold": 3, "type": kind })
}

fn port(number: u16) -> Json {
    json!({ "long_id": "00000000-0000-0000-0000-0000000000b2", "port": number, "is_default": true,
            "name": format!("p{number}"), "protocol": { "type": "TCP" },
            "service_name": null, "namespace": null })
}

fn http_host() -> Json {
    json!({ "domain_name": "app.example.com", "path": "/", "path_rewrite": "/$1", "service_name": "app",
            "service_port": 8080, "weight": 100, "grpc_service": "pkg.Service", "grpc_method": "Method" })
}

/// A probe carries exactly one type, so no single context can take every branch. The base context
/// sets an http readiness probe and an exec liveness probe; these variants cover the remaining
/// six combinations of the four branches across the two probes.
fn probe_variants() -> Vec<[(&'static str, Json); 2]> {
    let readiness = |kind: Json| ("service.readiness_probe.type", kind);
    let liveness = |kind: Json| ("service.liveness_probe.type", kind);
    let tcp = || json!({ "tcp": { "host": "127.0.0.1" }, "http": null, "exec": null, "grpc": null });
    let http = || json!({ "tcp": null, "http": { "path": "/health", "scheme": "HTTP" }, "exec": null, "grpc": null });
    let exec = || json!({ "tcp": null, "http": null, "exec": { "commands": ["sh", "-c", "true"] }, "grpc": null });
    let grpc = || json!({ "tcp": null, "http": null, "exec": null, "grpc": { "service": "pkg.Health" } });

    vec![
        [readiness(tcp()), liveness(grpc())],
        [readiness(grpc()), liveness(http())],
        [readiness(exec()), liveness(tcp())],
    ]
}

/// The KEDA autoscaling shape used by the manifest and literal-values cases.
fn keda_autoscaling() -> Json {
    json!({
        "type": "keda",
        "polling_interval_seconds": 30,
        "cooldown_period_seconds": 300,
        "fallback": { "failure_threshold": 3, "replicas": 1, "behavior": "static" },
        "scalers": [{ "scaler_type": "prometheus",
                      "metadata": { "serverAddress": "http://prometheus:9090", "threshold": "10" },
                      "raw_yaml": null,
                      "authentication_ref": { "name": "prom-auth" } }],
        "trigger_authentications": [{ "name": "prom-auth",
                                      "spec": null,
                                      "raw_yaml": "secretTargetRef:\n  - parameter: token\n    name: s\n    key: k" }],
    })
}

fn ctx(overrides: &[(&str, Json)]) -> Context {
    let mut value = base();
    for (path, replacement) in overrides {
        set(&mut value, path, replacement.clone());
    }
    Context::from_value(value).expect("context must be a JSON object")
}

#[test]
fn escaping_preserves_q_container_manifests() {
    assert_family_equivalent(
        "q-container",
        vec![
            ("deployment", ctx(&[])),
            ("deployment", ctx(&[("service.legacy_deployment_matchlabels", json!(true))])),
            (
                "deployment",
                ctx(&[
                    ("service.legacy_deployment_matchlabels", json!(true)),
                    ("service.legacy_deployment_from_scaleway", json!(true)),
                ]),
            ),
            ("secret", ctx(&[])),
            ("mounted_files_secret", ctx(&[])),
            (
                "horizontal_autoscaler",
                ctx(&[
                    ("service.max_instances", json!(3)),
                    ("service.advanced_settings.hpa_memory_average_utilization_percent", json!(70)),
                ]),
            ),
            ("pdb", ctx(&[("service.max_instances", json!(3))])),
            // the probe branches the base context does not take
            ("deployment", ctx(&probe_variants()[0])),
            ("deployment", ctx(&probe_variants()[1])),
            ("deployment", ctx(&probe_variants()[2])),
            // renders only with a default port; the layer 4 block covers the hostname annotation
            (
                "service",
                ctx(&[
                    ("service.default_port", json!(port(8080))),
                    ("service.ports", json!([port(8080)])),
                    (
                        "service.ports_layer4_public",
                        json!([{ "protocol": "TCP", "hostnames": ["tcp-test.example.com"], "ports": [port(5432)] }]),
                    ),
                ]),
            ),
            // Renders only for keda, and only with at least one scaler. The trigger
            // authentication uses `raw_yaml` rather than `spec`: the pre-escaping template piped
            // `spec` through `to_nice_yaml`, a filter registered nowhere, so that path could never
            // render and there is no previous behaviour to compare against.
            ("keda_autoscaling", ctx(&[("service.autoscaling", keda_autoscaling())])),
        ],
    );
}

#[test]
fn native_q_container_preserves_structured_keda_values_and_literal_templates() {
    let literal = r#"{{ fail "values must not be evaluated" }}"#;
    let spec = json!({"secretTargetRef": [{"parameter": "token", "name": "s", "key": "k"}]});
    let mut autoscaling = keda_autoscaling();
    autoscaling["trigger_authentications"][0]["spec"] = spec.clone();
    autoscaling["trigger_authentications"][0]["raw_yaml"] = Json::Null;
    autoscaling["scalers"][0]["metadata"] = Json::Null;
    autoscaling["scalers"][0]["raw_yaml"] = json!(format!("metadata:\n  query: '{literal}'\n  threshold: '10'"));
    let rendered = render_current("q-container", "keda_autoscaling", &ctx(&[("service.autoscaling", autoscaling)]))
        .expect("native Helm chart should render literal template text without executing it");
    let documents: Vec<Json> = serde_yaml::Deserializer::from_str(&rendered)
        .map(|document| Json::deserialize(document).expect("valid manifest"))
        .collect();
    let authentication = documents
        .iter()
        .find(|doc| doc["kind"] == "TriggerAuthentication")
        .expect("authentication");
    assert_eq!(authentication["spec"], spec);
    let scaled_object = documents
        .iter()
        .find(|doc| doc["kind"] == "ScaledObject")
        .expect("scaled object");
    assert_eq!(scaled_object["spec"]["triggers"][0]["metadata"]["query"], literal);
    assert_eq!(scaled_object["spec"]["triggers"][0]["metadata"]["threshold"], "10");
}

#[test]
fn escaping_preserves_q_job_manifests() {
    let job = [("service.type", json!("job"))];
    // job.j2.yaml renders only without a schedule, cronjob.j2.yaml only with one
    let one_shot = [
        ("service.type", json!("job")),
        ("service.cronjob_schedule", json!(null)),
    ];
    assert_family_equivalent(
        "q-job",
        vec![
            ("job", ctx(&one_shot)),
            ("cronjob", ctx(&job)),
            ("secret", ctx(&job)),
            ("pdb", ctx(&job)),
            ("rbac", ctx(&job)),
            ("mounted_files_config_map", ctx(&job)),
        ],
    );
}

#[test]
fn escaping_preserves_q_terraform_service_manifests() {
    let tf = [("service.type", json!("terraform"))];
    assert_family_equivalent(
        "q-terraform-service",
        vec![
            ("job", ctx(&tf)),
            ("secret", ctx(&tf)),
            ("pdb", ctx(&tf)),
            ("rbac", ctx(&tf)),
            ("pvc", ctx(&tf)),
        ],
    );
}

#[test]
fn escaping_preserves_single_template_charts() {
    let agentic = [("service.type", json!("agentic-workflow"))];
    let bedrock = [
        ("service.type", json!("agentic-workflow")),
        ("service.bedrock", json!({ "region": "eu-west-3" })),
        (
            "bedrock_credentials",
            json!([{ "key": "AWS_BEARER_TOKEN_BEDROCK", "value": "YmVkcm9jay10b2tlbg==", "is_secret": true }]),
        ),
    ];
    assert_family_equivalent(
        "q-agentic-workflow",
        vec![
            ("secret", ctx(&agentic)),
            ("job", ctx(&agentic)),
            ("prompt_config_map", ctx(&agentic)),
            ("secret", ctx(&bedrock)),
            ("job", ctx(&bedrock)),
        ],
    );
    assert_family_equivalent("q-external-secret", vec![("external_secret", ctx(&[]))]);
}

#[test]
fn escaping_preserves_q_ingress_tls_manifests() {
    let nginx = [
        ("k8s_remove_nginx", json!(false)),
        ("http_hosts_per_namespace_nginx", json!({ "app-ns": [http_host()] })),
        ("grpc_hosts_per_namespace_nginx", json!({ "app-ns": [http_host()] })),
    ];
    let mut no_rewrite = http_host();
    no_rewrite["path_rewrite"] = Json::Null;
    let null_rewrites = [
        ("k8s_remove_nginx", json!(false)),
        ("http_hosts_per_namespace_nginx", json!({"app-ns": [no_rewrite.clone()]})),
        ("grpc_hosts_per_namespace_nginx", json!({"app-ns": [no_rewrite.clone()]})),
    ];
    let mixed_rewrites = [
        ("k8s_remove_nginx", json!(false)),
        (
            "http_hosts_per_namespace_nginx",
            json!({"app-ns": [no_rewrite.clone(), http_host()]}),
        ),
        ("grpc_hosts_per_namespace_nginx", json!({"app-ns": [no_rewrite, http_host()]})),
    ];
    let gateway = [
        ("http_hosts_per_namespace_gateway", json!({ "app-ns": [http_host()] })),
        ("grpc_hosts_per_namespace_gateway", json!({ "app-ns": [http_host()] })),
        (
            "gateway_http_routes_per_namespace",
            json!({ "app-ns": [{ "hostnames": ["app.example.com"],
                                 "rules": [{ "path": "/", "path_rewrite": "/", "path_type": "PathPrefix",
                                             "service_name": "app", "service_port": 8080, "weight": 100 }] }] }),
        ),
    ];
    let both: Vec<(&str, Json)> = nginx.iter().chain(gateway.iter()).cloned().collect();
    let compression_enabled = [("cluster_envoy_enable_compression", json!(true))];
    let both_with_compression: Vec<(&str, Json)> = both.iter().chain(compression_enabled.iter()).cloned().collect();
    let mut shared_certificate_policy = both.clone();
    shared_certificate_policy.push((
        "certificate_alternative_names",
        json!([{ "domain": "app.example.com" }, { "domain": "alias.example.com" }]),
    ));

    assert_family_equivalent(
        "q-ingress-tls",
        vec![
            ("ingress-http", ctx(&nginx)),
            ("ingress-grpc", ctx(&nginx)),
            ("ingress-http", ctx(&null_rewrites)),
            ("ingress-grpc", ctx(&null_rewrites)),
            ("ingress-http", ctx(&mixed_rewrites)),
            ("ingress-grpc", ctx(&mixed_rewrites)),
            // renders only when basic auth is configured
            (
                "secret-htaccess",
                ctx(&[("advanced_settings.network_ingress_basic_auth_env_var", json!("BASIC_AUTH"))]),
            ),
            ("gateway-http-route", ctx(&both)),
            ("gateway-grpc-route", ctx(&both)),
            ("gateway-http-route-filter", ctx(&both)),
            ("gateway-http-route-envoy-backend-traffic-policy", ctx(&both_with_compression)),
            ("gateway-grpc-route-envoy-backend-traffic-policy", ctx(&both_with_compression)),
            ("gateway-http-route-envoy-security-policy", ctx(&both)),
            ("gateway-grpc-route-envoy-security-policy", ctx(&both)),
            ("gateway-http-route-envoy-error-pages-configmap", ctx(&both)),
            ("gateway-client-traffic-policy", ctx(&shared_certificate_policy)),
            ("listenerset", ctx(&both)),
        ],
    );
}

/// Deployment charts must contain only native Helm templates; frozen fixtures are excluded.
#[test]
fn service_charts_are_native_helm() {
    let mut template_count = 0;
    for directory in ["lib/common/charts", "lib/common/services"] {
        for entry in walkdir::WalkDir::new(Path::new(env!("CARGO_MANIFEST_DIR")).join(directory)) {
            let entry = entry.expect("chart entry");
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            assert!(
                !entry.file_name().to_string_lossy().contains(".j2."),
                "legacy Tera file in service chart: {}",
                path.display()
            );
            if !matches!(path.extension().and_then(|ext| ext.to_str()), Some("yaml" | "yml" | "tpl")) {
                continue;
            }
            let content = std::fs::read_to_string(path).expect("read chart template");
            assert!(
                !content.contains("{%") && !content.contains("{#"),
                "Tera directive in service chart: {}",
                path.display()
            );
            template_count += 1;
        }
    }
    assert!(template_count > 0, "no chart templates were checked");
}
