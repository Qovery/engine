//! Structural regression guards for native service Helm charts.
//!
//! Deployment values remain data: never run them through `tpl`, reintroduce Tera,
//! or put arbitrary strings inside hand-written YAML quotes. Rendering tests cover
//! escaping and intentional raw YAML; these checks prevent unsafe template patterns.

use regex::Regex;
use std::path::{Path, PathBuf};

fn templates() -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("readable chart directory") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else if matches!(path.extension().and_then(|s| s.to_str()), Some("yaml" | "tpl")) {
                found.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("lib/common");
    let mut paths = vec![];
    walk(&root.join("charts"), &mut paths);
    for database in ["mysql", "postgresql", "mongodb", "redis"] {
        walk(&root.join("services").join(database), &mut paths);
    }
    paths.sort();
    paths
}

/// Extract Go actions without treating closing braces in strings or comments as delimiters.
fn actions(text: &str) -> Vec<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let mut result = vec![];
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find("{{") {
        let start = cursor + offset;
        let mut end = start + 2;
        let mut quote = None;
        let mut comment = false;
        while end + 1 < bytes.len() {
            if comment {
                if &bytes[end..end + 2] == b"*/" {
                    comment = false;
                    end += 2;
                } else {
                    end += 1;
                }
            } else if let Some(delimiter) = quote {
                if bytes[end] == b'\\' && delimiter != b'`' {
                    end += 2;
                } else {
                    if bytes[end] == delimiter {
                        quote = None;
                    }
                    end += 1;
                }
            } else if &bytes[end..end + 2] == b"/*" {
                comment = true;
                end += 2;
            } else if matches!(bytes[end], b'"' | b'`' | b'\'') {
                quote = Some(bytes[end]);
                end += 1;
            } else if &bytes[end..end + 2] == b"}}" {
                result.push((start, end + 2, &text[start + 2..end]));
                break;
            } else {
                end += 1;
            }
        }
        if end + 1 >= bytes.len() {
            break;
        }
        cursor = end + 2;
    }
    result
}

fn violations(text: &str) -> Vec<String> {
    let comments = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let literals = Regex::new(r#""(?:\\.|[^"\\])*"|`[^`]*`|'(?:\\.|[^'\\])*'"#).unwrap();
    let forbidden = Regex::new(r"(?:^|[\s|(])(tpl|yaml_encode)(?:[\s|)]|$)").unwrap();
    let quoted_scalar = Regex::new(r#"^\s*(?:-\s*)?[^:]+:\s*["']"#).unwrap();
    let mut errors = vec![];
    // Tera delimiters inside a Go string are literal data rather than Tera syntax.
    let mut outside_actions = String::new();
    let mut cursor = 0;
    for (start, end, body) in actions(text) {
        outside_actions.push_str(&text[cursor..start]);
        cursor = end;
        let code = literals.replace_all(body, "");
        let code = comments.replace_all(&code, "");
        if forbidden.is_match(&code) {
            errors.push(format!("template evaluation or legacy filter: {body}"));
        }
    }
    outside_actions.push_str(&text[cursor..]);
    if outside_actions.contains("{%") || outside_actions.contains("{#") {
        errors.push("legacy Tera syntax".to_string());
    }
    for (line_number, line) in text.lines().enumerate() {
        if !quoted_scalar.is_match(line) {
            continue;
        }
        for (_, _, body) in actions(line) {
            let expression = body.trim();
            // These exact expressions are typed numbers or engine-generated UUIDs.
            // Arbitrary field suffixes must not grant exemptions to free-form strings.
            if !matches!(
                expression,
                "$port.port"
                    | "$.Values.id"
                    | "$.Values.advanced_settings.network_ingress_proxy_body_size_mb"
                    | "$.Values.advanced_settings.network_ingress_proxy_buffer_size_kb"
                    | "(mul $.Values.advanced_settings.network_ingress_proxy_buffer_size_kb 2)"
            ) {
                errors.push(format!(
                    "line {}: hand-quoted YAML interpolation: {expression}",
                    line_number + 1
                ));
            }
        }
    }
    errors
}

#[test]
fn service_charts_keep_deployment_values_as_data() {
    let paths = templates();
    assert!(paths.len() > 40, "service chart scan unexpectedly empty");
    let mut errors = vec![];
    let mut interpolations = 0;
    for path in paths {
        let text = std::fs::read_to_string(&path).unwrap();
        interpolations += actions(&text).len();
        if path.to_string_lossy().contains(".j2.") {
            errors.push(format!("{}: legacy Tera file", path.display()));
        }
        errors.extend(
            violations(&text)
                .into_iter()
                .map(|error| format!("{}: {error}", path.display())),
        );
    }
    assert!(interpolations > 400, "only {interpolations} Helm actions scanned");
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn native_guard_rejects_unsafe_patterns() {
    for text in [
        "{{ tpl $.Values.payload . }}",
        "{{ $payload := $.Values.payload }}{{ $payload | tpl . }}",
        "{% if service.name %}",
        "{{ value | yaml_encode }}",
        "image: \"{{ $.Values.service.image_full }}\"",
        "name: 'prefix-{{ $value }}'",
        "name: \"{{ $value | quote }}\"",
        "name: \"{{ $port.port }}-{{ $value }}\"",
        "{{ printf \"}}\" | tpl . }}",
    ] {
        assert!(!violations(text).is_empty(), "accepted unsafe pattern: {text}");
    }
}

#[test]
fn native_guard_accepts_safe_encoding_and_literal_text() {
    for text in [
        "image: {{ $.Values.service.image_full | quote }}",
        "{{ $key | toJson }}: {{ $value | toJson }}",
        "name: {{ printf \"prefix-%s\" $value | quote }}",
        "name: \"p{{ $port.port }}\"",
        "{{ printf \"tpl is literal text\" | quote }}",
        "{{ printf \"}} tpl {%\" | quote }}",
        "{{/* tpl }} in a comment is not evaluated */}}",
        "      add_header {{ $name }} \"{{ include \"qovery.nginxHeaderValue\" $value }}\";",
    ] {
        assert!(violations(text).is_empty(), "rejected safe pattern: {text}");
    }
}
