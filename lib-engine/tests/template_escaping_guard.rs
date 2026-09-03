//! Default-deny guard over the service chart templates.
//!
//! Every `{{ … }}` in a chart must either pass through an escaping filter or name a value that
//! cannot carry a YAML break-out — an identifier, a number, an enum, a base64 payload. Anything
//! else fails this test, so a new interpolation has to be classified before it can merge.
//!
//! This exists because the escaping pass was twice reopened by unrelated work: a mount path and a
//! secret `data:` block arrived in `q-agentic-workflow` after the sweep and shipped raw. Review
//! caught them; this catches the next one.
//!
//! When this test fails, the fix is one of:
//!   - the value comes from the deployment request: pipe it through `yaml_encode`;
//!   - the value cannot break out: add it below, with the reason it is safe.

use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Filters that make an interpolation safe to emit.
const ESCAPING_FILTERS: &[&str] = &[
    "yaml_encode",
    "base64_encode",
    "nginx_header_name_escape",
    "nginx_header_value_escape",
    "hcl_string",
    "hcl_heredoc",
    "json_encode",
];

/// Suffixes that identify a value whose type cannot hold a quote or a newline: numbers, durations,
/// percentages, ports, booleans, enums, engine identifiers and base64 payloads.
const SAFE_SUFFIXES: &[&str] = &[
    "_id",
    "_b64",
    "_seconds",
    "_in_milli",
    "_in_mib",
    "_in_gib",
    "_in_sec",
    "_percent",
    "_mb",
    "_kb",
    "_rpm",
    "_rps",
    "_port",
    "_retries",
    "_multiplier",
    "_connections",
    "_requests",
    "_instances",
    "_threshold",
    "_index",
    "_index0",
    "_limit",
    "_policy",
    "_enable",
    "_redirect",
    "_type",
    "_scheme",
    "_restart",
    "_history_limit",
    "_after_finished",
    "_regex_path",
    "_root_filesystem",
    "_account_token",
    "_zone",
    "_ndots",
    "kube_name",
    ".replicas",
    ".port",
    ".weight",
    ".protocol",
];

/// Prefixes whose whole subtree is numeric or enum — probe timings and thresholds, loop counters.
const SAFE_PREFIXES: &[&str] = &["loop.", "service.readiness_probe.", "service.liveness_probe."];

/// Everything else that is safe, and why. Anything customer-controlled belongs in `yaml_encode`,
/// not here.
const SAFE_EXPRESSIONS: &[&str] = &[
    // engine-generated identifiers and kube names
    "namespace",
    "namespace_key",
    "sanitized_name",
    "id",
    "long_id",
    "service.name",
    "service.short_id",
    "service.long_id",
    "service.kube_name",
    "service.type",
    "service_type",
    "service.version",
    "service.image_full",
    "service.image_tag_label",
    "service.default_port",
    "service.gpu_request",
    "external_secret.secret_name",
    "external_secret.store_name",
    "registry.secret_name",
    "backend_config.secret_name",
    "entry.volume_name",
    "mounted_file.kube_name",
    "host.service_name",
    "rule.service_name",
    "s.id",
    "s.long_id",
    // sanitised or typed loop values
    "safe_header_name",
    "code",
    "trigger",
    "status_code",
    "annotation",
    "port.protocol.type",
    "l4_ports.protocol",
    "publicly_accessible",
    // engine-composed payloads, already encoded upstream
    "basic_auth_htaccess",
    "registry.docker_json_config",
    "line",
    // `{% set %}` locals derived from numeric advanced settings
    "service_request_timeout",
    "service_idle_timeout",
    "service_max_stream_duration",
    "cluster_request_timeout",
    "cluster_idle_timeout",
    "cluster_max_stream_duration",
    "num_routes",
    "route_index",
    "end_idx_calc",
    // Raw passthroughs by design: these fields exist to inject YAML or nginx config, so escaping
    // them would defeat their purpose. A `{{` inside them still reaches Helm — tracked separately.
    "scaler.raw_yaml",
    "trigger_auth.raw_yaml",
    "nginx_ingress_controller_configuration_snippet",
    "nginx_ingress_controller_server_snippet",
];

fn charts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("lib/common/charts")
}

fn templates() -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("chart directory must be readable") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.to_string_lossy().ends_with(".j2.yaml") {
                found.push(path);
            }
        }
    }
    let mut found = vec![];
    walk(&charts_dir(), &mut found);
    found.sort();
    found
}

fn is_safe(expression: &str, filters: &str) -> bool {
    if ESCAPING_FILTERS.iter().any(|f| filters.contains(f)) {
        return true;
    }
    if SAFE_EXPRESSIONS.contains(&expression)
        || SAFE_PREFIXES.iter().any(|p| expression.starts_with(p))
        || SAFE_SUFFIXES.iter().any(|s| expression.ends_with(s))
    {
        return true;
    }
    // arithmetic on a numeric setting, e.g. `buffer_size_kb * 2`, `route_index + 1`
    Regex::new(r"[-+*/]\s*\d+$").expect("valid regex").is_match(expression)
}

#[test]
fn every_chart_interpolation_is_escaped_or_classified_safe() {
    // `{% raw %}` carries sprig for Helm's own pass, not Tera interpolation
    let raw_block = Regex::new(r"(?s)\{%-?\s*raw\s*-?%\}.*?\{%-?\s*endraw\s*-?%\}").expect("valid regex");
    let interpolation = Regex::new(r"\{\{(.+?)\}\}").expect("valid regex");

    let mut offenders: BTreeSet<String> = BTreeSet::new();
    let mut scanned = 0usize;

    for path in templates() {
        let text = std::fs::read_to_string(&path).expect("template must be readable");
        let text = raw_block.replace_all(&text, "");
        let relative = path.strip_prefix(charts_dir()).unwrap_or(&path).display().to_string();

        for (number, line) in text.lines().enumerate() {
            for capture in interpolation.captures_iter(line) {
                let body = capture[1].trim();
                let (expression, filters) = body.split_once('|').unwrap_or((body, ""));
                scanned += 1;
                if !is_safe(expression.trim(), filters) {
                    offenders.insert(format!("{relative}:{}  {{{{ {body} }}}}", number + 1));
                }
            }
        }
    }

    assert!(scanned > 400, "only {scanned} interpolations scanned — the walk is broken");
    assert!(
        offenders.is_empty(),
        "{} chart interpolation(s) are neither escaped nor classified safe.\n\
         Pipe deployment-supplied values through `yaml_encode`, or add the expression to \
         SAFE_EXPRESSIONS with the reason it cannot break out:\n\n{}",
        offenders.len(),
        offenders.into_iter().collect::<Vec<_>>().join("\n")
    );
}
