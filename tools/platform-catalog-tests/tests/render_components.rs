use base64::Engine;
use platform_catalog_tests::{
    contains_key, contains_string, contains_string_fragment, document_by_kind_and_name, document_kind, helm_template,
    parse_yaml_documents, parse_yaml_file, repository_path, yaml_path, yaml_string,
};
use serde_yaml::Value;
use std::fs;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn values(path: &str) -> PathBuf {
    repository_path(path)
}

fn render(
    release: &str,
    chart: &str,
    namespace: &str,
    value_files: &[PathBuf],
    extra_arguments: &[&str],
) -> Vec<Value> {
    parse_yaml_documents(&helm_template(release, chart, namespace, value_files, extra_arguments))
}

fn write_runtime_values(content: &str) -> NamedTempFile {
    let file = NamedTempFile::new().expect("runtime values file must be created");
    fs::write(file.path(), content).expect("runtime values file must be writable");
    file
}

fn any_document_contains(documents: &[Value], expected: &str) -> bool {
    documents.iter().any(|document| contains_string(document, expected))
}

fn any_document_contains_fragment(documents: &[Value], expected: &str) -> bool {
    documents
        .iter()
        .any(|document| contains_string_fragment(document, expected))
}

fn any_document_contains_key(documents: &[Value], expected: &str) -> bool {
    documents.iter().any(|document| contains_key(document, expected))
}

fn assert_workload_scheduling_matches(
    documents: &[Value],
    kind: &str,
    name: &str,
    overlay: &Value,
    overlay_section: &str,
) {
    let workload = document_by_kind_and_name(documents, kind, name)
        .unwrap_or_else(|| panic!("missing rendered Loki workload {kind}/{name}"));
    let pod_spec = yaml_path(workload, &["spec", "template", "spec"])
        .unwrap_or_else(|| panic!("missing pod spec for rendered Loki workload {kind}/{name}"));

    assert_eq!(
        yaml_path(pod_spec, &["affinity", "nodeAffinity"]),
        yaml_path(overlay, &[overlay_section, "affinity", "nodeAffinity"]),
        "unexpected node affinity for rendered Loki workload {kind}/{name}"
    );
    assert_eq!(
        yaml_path(pod_spec, &["tolerations"]),
        yaml_path(overlay, &[overlay_section, "tolerations"]),
        "unexpected tolerations for rendered Loki workload {kind}/{name}"
    );
    assert!(
        yaml_path(pod_spec, &["affinity", "podAntiAffinity"]).is_some(),
        "missing chart pod anti-affinity for rendered Loki workload {kind}/{name}"
    );
}

#[test]
fn priority_classes_render_with_expected_names() {
    let documents = render(
        "qovery-priority-class",
        "lib-engine/lib/common/bootstrap/charts/qovery-priority-class",
        "qovery",
        &[values(
            "platform-catalog/components/qovery-priority-class/config/static-values/base.yaml",
        )],
        &[],
    );

    assert!(document_by_kind_and_name(&documents, "PriorityClass", "qovery-high-priority").is_some());
    assert!(document_by_kind_and_name(&documents, "PriorityClass", "qovery-standard-priority").is_some());
}

#[test]
fn cluster_agent_does_not_receive_the_legacy_loki_url() {
    let documents = render(
        "cluster-agent",
        "lib-engine/lib/common/bootstrap/charts/qovery-cluster-agent",
        "qovery",
        &[
            values("platform-catalog/components/cluster-agent/config/static-values/base.yaml"),
            values("platform-catalog/components/cluster-agent/config/runtime-values/managed-values.yaml"),
        ],
        &[],
    );

    assert!(!any_document_contains(&documents, "LOKI_URL"));
}

#[test]
fn alloy_renders_the_expected_public_image_resources_and_loki_pipeline() {
    let value_file = values("platform-catalog/components/alloy/config/static-values/base.yaml");
    let documents = render(
        "alloy",
        "lib-engine/lib/common/bootstrap/charts/alloy",
        "qovery",
        std::slice::from_ref(&value_file),
        &[],
    );

    assert!(!any_document_contains_fragment(&documents, "{% raw %}"));
    assert!(!any_document_contains_fragment(&documents, "{% endraw %}"));
    assert!(any_document_contains_fragment(
        &documents,
        "template = \"{{ if .Value }}{{ ToLower .Value }}{{ end }}\""
    ));
    for expected in [
        "qovery_com_deployment_id",
        "http://loki-gateway.qovery.svc/loki/api/v1/push",
    ] {
        assert!(any_document_contains_fragment(&documents, expected));
    }
    for expected in [
        "GOMEMLIMIT",
        "450MiB",
        "public.ecr.aws/r3m4q3r9/pub-mirror-alloy@sha256:41c41849989b7e054ccbadc17938ee1e5592fe26bfbc56ef3ffc109c0b0b2739",
        "100m",
        "128Mi",
        "512Mi",
    ] {
        assert!(any_document_contains(&documents, expected), "missing Alloy value {expected}");
    }

    let static_values = parse_yaml_file(value_file);
    assert!(yaml_path(&static_values, &["namespace"]).is_none());
}

#[test]
fn loki_supports_single_binary_and_simple_scalable_modes() {
    let chart = "lib-engine/lib/common/bootstrap/charts/loki";
    let value_file = values("platform-catalog/components/loki/config/static-values/base.yaml");
    let single_binary = render(
        "loki",
        chart,
        "qovery",
        std::slice::from_ref(&value_file),
        &[
            "--set",
            "deploymentMode=SingleBinary",
            "--set",
            "backend.replicas=0",
            "--set",
            "read.replicas=0",
            "--set",
            "write.replicas=0",
        ],
    );
    assert!(document_by_kind_and_name(&single_binary, "Service", "loki-gateway").is_some());
    for expected in [
        "qovery-high-priority",
        "public.ecr.aws/r3m4q3r9/pub-mirror-loki@sha256:3c8fd3570dd9219951a60d3f919c7f31923d10baee578b77bc26c4a0b32d092d",
        "docker.io/nginxinc/nginx-unprivileged@sha256:0c79d56aee561a1d81c63f00eee5fb5fe29279560cdc55e91425133104c7fbe6",
    ] {
        assert!(any_document_contains(&single_binary, expected));
    }
    assert!(any_document_contains_fragment(
        &single_binary,
        "proxy_pass       http://loki.qovery.svc.cluster.local:3100$request_uri;"
    ));

    let simple_scalable_values = repository_path(format!("{chart}/simple-scalable-values.yaml"));
    let simple_scalable = render(
        "loki",
        chart,
        "qovery",
        &[value_file, simple_scalable_values],
        &["--set", "loki.storage.type=s3"],
    );
    assert!(document_by_kind_and_name(&simple_scalable, "Service", "loki-gateway").is_some());
    assert!(any_document_contains_fragment(
        &simple_scalable,
        "proxy_pass       http://loki-write.qovery.svc.cluster.local:3100$request_uri;"
    ));
}

#[test]
fn loki_qovery_karpenter_capability_overlay_matches_legacy_scheduling_and_renders() {
    let chart = "lib-engine/lib/common/bootstrap/charts/loki";
    let base_values = values("platform-catalog/components/loki/config/static-values/base.yaml");
    let capability_overlay =
        values("platform-catalog/components/loki/config/static-values/overlays/qovery-karpenter.yaml");
    let legacy_overlay = values("lib-engine/lib/common/bootstrap/chart_values/loki_with_karpenter.yaml");
    let overlay_values = parse_yaml_file(&capability_overlay);
    let legacy_values = parse_yaml_file(legacy_overlay);

    for section in ["write", "read", "backend", "singleBinary"] {
        assert_eq!(
            yaml_path(&overlay_values, &[section]),
            yaml_path(&legacy_values, &[section]),
            "Loki Qovery Karpenter scheduling for {section} must remain compatible with Engine v1"
        );
    }

    let single_binary_documents = render(
        "loki",
        chart,
        "qovery",
        &[base_values.clone(), capability_overlay.clone()],
        &[
            "--set",
            "deploymentMode=SingleBinary",
            "--set",
            "singleBinary.replicas=1",
            "--set",
            "backend.replicas=0",
            "--set",
            "read.replicas=0",
            "--set",
            "write.replicas=0",
        ],
    );

    for (kind, name, overlay_section) in [
        ("StatefulSet", "loki", "singleBinary"),
        ("Deployment", "loki-gateway", "gateway"),
    ] {
        assert_workload_scheduling_matches(&single_binary_documents, kind, name, &overlay_values, overlay_section);
    }

    // Representative subset of Source 3 values emitted by compile.pkl for HA S3 with custom resources.
    let source_3_values = write_runtime_values(
        r#"deploymentMode: SimpleScalable
loki:
  storage:
    type: s3
    bucketNames:
      chunks: qovery-loki
      ruler: qovery-loki
      admin: qovery-loki
singleBinary:
  replicas: 0
write:
  replicas: 3
  resources:
    requests:
      cpu: 750m
      memory: 1Gi
read:
  replicas: 3
  resources:
    requests:
      cpu: 500m
      memory: 512Mi
backend:
  replicas: 3
  resources:
    requests:
      cpu: 250m
      memory: 512Mi
gateway:
  replicas: 3
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
"#,
    );
    let simple_scalable_documents = render(
        "loki",
        chart,
        "qovery",
        &[base_values, capability_overlay, source_3_values.path().to_path_buf()],
        &[],
    );

    for (kind, name, overlay_section) in [
        ("StatefulSet", "loki-write", "write"),
        ("Deployment", "loki-read", "read"),
        ("StatefulSet", "loki-backend", "backend"),
        ("Deployment", "loki-gateway", "gateway"),
    ] {
        assert_workload_scheduling_matches(&simple_scalable_documents, kind, name, &overlay_values, overlay_section);
    }
}

#[test]
fn cert_manager_disables_gateway_and_service_monitor_features() {
    let value_file = values("platform-catalog/components/cert-manager/config/static-values/base.yaml");
    let documents = render(
        "cert-manager",
        "lib-engine/lib/common/bootstrap/charts/cert-manager",
        "qovery",
        std::slice::from_ref(&value_file),
        &[],
    );

    assert!(
        documents
            .iter()
            .any(|document| document_kind(document) == Some("CustomResourceDefinition"))
    );
    assert!(any_document_contains_fragment(&documents, "enableGatewayAPI: false"));
    assert!(any_document_contains_fragment(&documents, "enableGatewayAPIListenerSet: false"));
    assert!(
        !documents
            .iter()
            .any(|document| document_kind(document) == Some("ServiceMonitor"))
    );

    let static_values = parse_yaml_file(value_file);
    assert_eq!(
        yaml_string(&static_values, &["global", "leaderElection", "namespace"]),
        Some("qovery")
    );
}

#[test]
fn qovery_webhook_uses_the_public_image_without_a_registry_secret() {
    let value_file = values("platform-catalog/components/qovery-cert-manager-webhook/config/static-values/base.yaml");
    let documents = render(
        "qovery-cert-manager-webhook",
        "lib-engine/lib/common/bootstrap/charts/qovery-cert-manager-webhook",
        "qovery",
        std::slice::from_ref(&value_file),
        &[],
    );

    assert!(any_document_contains(
        &documents,
        "public.ecr.aws/r3m4q3r9/cert-manager-webhook-qovery:84a4b276"
    ));
    assert!(!any_document_contains_key(&documents, "imagePullSecrets"));
    assert!(!documents.iter().any(|document| {
        document_kind(document) == Some("Secret")
            && yaml_string(document, &["type"]) == Some("kubernetes.io/dockerconfigjson")
    }));

    let static_values = parse_yaml_file(value_file);
    assert_eq!(yaml_string(&static_values, &["certManager", "namespace"]), Some("qovery"));
}

#[test]
fn qovery_dns_consumers_keep_split_and_combined_endpoint_contracts() {
    let cert_manager_values = parse_yaml_file(repository_path(
        "platform-catalog/components/cert-manager-configs/config/runtime-values/managed-values.yaml",
    ));
    assert_eq!(
        yaml_string(&cert_manager_values, &["provider", "pdns", "apiUrl"]),
        Some("${dns.qovery.apiUrl}")
    );
    assert_eq!(
        yaml_string(&cert_manager_values, &["provider", "pdns", "apiPort"]),
        Some("${dns.qovery.apiPort}")
    );

    let external_dns_values = parse_yaml_file(repository_path(
        "platform-catalog/components/external-dns/config/runtime-values/managed-values.yaml",
    ));
    assert_eq!(
        yaml_string(&external_dns_values, &["extraArgs", "pdns-server"]),
        Some("${dns.qovery.apiEndpoint}")
    );
    assert_eq!(
        yaml_string(
            &external_dns_values,
            &["podAnnotations", "qovery.com/external-dns-credential-revision"]
        ),
        Some("${cluster.jwtKid}")
    );
}

#[test]
fn external_dns_secret_contains_only_the_encoded_provider_token() {
    let dns_token = "slice-4-8-token";
    let encoded_token = base64::engine::general_purpose::STANDARD.encode(dns_token);
    let output = helm_template(
        "renamed-external-dns-secret-release",
        "lib-engine/lib/common/bootstrap/charts/external-dns-secret",
        "qovery",
        &[values(
            "platform-catalog/components/external-dns-secret/config/static-values/base.yaml",
        )],
        &["--set-string", &format!("pdns.apiKey={dns_token}")],
    );
    let documents = parse_yaml_documents(&output);
    let secret = document_by_kind_and_name(&documents, "Secret", "external-dns-secret")
        .expect("external DNS Secret must be rendered");

    assert_eq!(yaml_string(secret, &["data", "pdns_api_key"]), Some(encoded_token.as_str()));

    let static_values = parse_yaml_file(repository_path(
        "platform-catalog/components/external-dns-secret/config/static-values/base.yaml",
    ));
    assert!(yaml_path(&static_values, &["namespace"]).is_none());
    assert_eq!(yaml_string(&static_values, &["fullnameOverride"]), Some("external-dns-secret"));
}

#[test]
fn external_dns_is_service_only_and_reloads_when_credentials_rotate() {
    let runtime_values = write_runtime_values(
        "domainFilters:\n  - slice-4-8.example.com\ntxtOwnerId: 11111111-1111-1111-1111-111111111111\nextraArgs:\n  pdns-server: https://dns.example.com:443\npodAnnotations:\n  qovery.com/external-dns-credential-revision: revision-1\n",
    );
    let documents = render(
        "external-dns",
        "lib-engine/lib/common/bootstrap/charts/external-dns",
        "qovery",
        &[
            values("platform-catalog/components/external-dns/config/static-values/base.yaml"),
            runtime_values.path().to_owned(),
        ],
        &[],
    );

    for expected in [
        "--source=service",
        "--provider=pdns",
        "--pdns-server=https://dns.example.com:443",
        "external-dns-secret",
        "revision-1",
    ] {
        assert!(
            any_document_contains(&documents, expected),
            "missing ExternalDNS value {expected}"
        );
    }
    assert!(!any_document_contains(&documents, "--source=ingress"));
    assert!(!any_document_contains(&documents, "--source=gateway"));
    assert!(
        !documents
            .iter()
            .any(|document| document_kind(document) == Some("ServiceMonitor"))
    );
}

#[test]
fn cert_manager_configs_render_dns01_resources_without_leaking_the_token() {
    let dns_token = "slice-4-8-token";
    let encoded_token = base64::engine::general_purpose::STANDARD.encode(dns_token);
    let runtime_values = write_runtime_values(&format!(
        "managedDns:\n  - slice-4-8.example.com\nacme:\n  letsEncrypt:\n    emailReport: dns@example.com\n    acmeUrl: https://acme-staging-v02.api.letsencrypt.org/directory\nprovider:\n  pdns:\n    apiUrl: https://dns.example.com\n    apiPort: \"443\"\n    apiKey: {dns_token}\n"
    ));
    let documents = render(
        "cert-manager-configs",
        "lib-engine/lib/common/bootstrap/charts/cert-manager-configs",
        "qovery",
        &[
            values("platform-catalog/components/cert-manager-configs/config/static-values/base.yaml"),
            runtime_values.path().to_owned(),
        ],
        &[],
    );

    let issuer = document_by_kind_and_name(&documents, "ClusterIssuer", "letsencrypt-qovery")
        .expect("ClusterIssuer must be rendered");
    assert_eq!(
        yaml_string(issuer, &["spec", "acme", "server"]),
        Some("https://acme-staging-v02.api.letsencrypt.org/directory")
    );
    assert!(any_document_contains(&documents, "slice-4-8.example.com"));
    assert!(
        documents
            .iter()
            .any(|document| document_kind(document) == Some("Certificate"))
    );
    assert!(any_document_contains(&documents, "*.slice-4-8.example.com"));
    assert!(!any_document_contains_key(&documents, "http01"));
    assert!(
        !documents
            .iter()
            .any(|document| document_kind(document) == Some("ReferenceGrant"))
    );

    for document in documents
        .iter()
        .filter(|document| document_kind(document) != Some("Secret"))
    {
        assert!(!contains_string_fragment(document, dns_token));
        assert!(!contains_string_fragment(document, &encoded_token));
    }

    let static_values = parse_yaml_file(repository_path(
        "platform-catalog/components/cert-manager-configs/config/static-values/base.yaml",
    ));
    assert_eq!(yaml_string(&static_values, &["namespace"]), Some("qovery"));
}
