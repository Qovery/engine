use base64::Engine;
use base64::engine::general_purpose;
use serde_json::Value;
use std::collections::HashMap;
use tera::{Context, Error, Tera, try_get_value};

// TODO(benjaminch): this should be an external crate
/// This file to declare custom functions / filters and stuff for tera
/// documentation => https://keats.github.io/tera/docs/#advanced-usage
pub trait TeraFilter<'a> {
    fn name() -> &'a str;
    fn implementation() -> fn(&Value, &HashMap<String, Value>) -> Result<Value, Error>;
}

/// Encodes string value to base 64.
pub struct Base64EncodeFilter {}

impl Base64EncodeFilter {
    fn base64_encode(s: &str) -> String {
        general_purpose::STANDARD.encode(s)
    }
}

impl<'a> TeraFilter<'a> for Base64EncodeFilter {
    fn name() -> &'a str {
        "base64_encode"
    }

    fn implementation() -> fn(&Value, &HashMap<String, Value>) -> Result<Value, Error> {
        |value: &Value, _: &HashMap<String, Value>| -> Result<Value, Error> {
            let s = try_get_value!("base64_encode", "value", String, value);
            Ok(Value::String(Base64EncodeFilter::base64_encode(&s)))
        }
    }
}

/// Escapes a string so it can be safely embedded inside a double-quoted HCL string literal.
/// Escapes `\`, `"`, HCL interpolation markers `${` / `%{`, and control chars (\n, \r, \t).
pub struct HclStringEscapeFilter {}

impl HclStringEscapeFilter {
    fn escape_chars(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace("${", "$${")
            .replace("%{", "%%{")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }
}

impl<'a> TeraFilter<'a> for HclStringEscapeFilter {
    fn name() -> &'a str {
        "hcl_string"
    }

    fn implementation() -> fn(&Value, &HashMap<String, Value>) -> Result<Value, Error> {
        |value: &Value, _: &HashMap<String, Value>| -> Result<Value, Error> {
            let s = try_get_value!("hcl_string", "value", String, value);
            Ok(Value::String(HclStringEscapeFilter::escape_chars(&s)))
        }
    }
}

/// Escapes a multi-line string for embedding inside an HCL heredoc (`<<-EOT ... EOT`).
/// Heredocs preserve raw bytes but still parse `${...}` and `%{...}` interpolation —
/// escape only those.
pub struct HclHeredocEscapeFilter {}

impl HclHeredocEscapeFilter {
    fn escape_chars(s: &str) -> String {
        s.replace("${", "$${").replace("%{", "%%{")
    }
}

impl<'a> TeraFilter<'a> for HclHeredocEscapeFilter {
    fn name() -> &'a str {
        "hcl_heredoc"
    }

    fn implementation() -> fn(&Value, &HashMap<String, Value>) -> Result<Value, Error> {
        |value: &Value, _: &HashMap<String, Value>| -> Result<Value, Error> {
            let s = try_get_value!("hcl_heredoc", "value", String, value);
            Ok(Value::String(HclHeredocEscapeFilter::escape_chars(&s)))
        }
    }
}

/// Encodes any value as a YAML node — usable both as a mapping key and as a value.
/// Strings come out double-quoted, everything else as compact JSON, which is a valid
/// YAML 1.2 subset. A `"`, a newline or a control char in customer input therefore
/// cannot terminate the node and inject sibling fields into the manifest.
///
/// Call sites must NOT wrap the interpolation in their own quotes.
///
/// Helm re-renders the manifest through Go text/template before applying it, so a
/// literal `{{` reaching the file is evaluated as a template action (`lookup` reads
/// any object the engine can read). The second brace is emitted as its YAML unicode
/// escape instead: the delimiter never forms in the file Helm reads, and the parser
/// decodes it back to a brace, leaving the applied value unchanged.
///
/// That holds for non-string values too, even though they come out as plain scalars or flow
/// collections rather than quoted scalars: compact JSON never places two `{` side by side outside
/// a string, so a `{{` can only reach the output from inside a nested quoted scalar, and those
/// decode the escape wherever they sit.
pub struct YamlEncodeFilter {}

impl YamlEncodeFilter {
    fn encode(value: &Value) -> String {
        // Compact JSON never puts two `{` side by side outside a string literal,
        // so this only ever rewrites braces coming from the input itself.
        value.to_string().replace("{{", "{\\u007b")
    }
}

impl<'a> TeraFilter<'a> for YamlEncodeFilter {
    fn name() -> &'a str {
        "yaml_encode"
    }

    fn implementation() -> fn(&Value, &HashMap<String, Value>) -> Result<Value, Error> {
        |value: &Value, _: &HashMap<String, Value>| -> Result<Value, Error> {
            Ok(Value::String(YamlEncodeFilter::encode(value)))
        }
    }
}

/// Sanitizes an HTTP header name for interpolation into an nginx directive.
/// Keeps the RFC 9110 token characters that are inert in an nginx config token, and drops the
/// rest: whitespace, `;`, `#`, `$` and quotes would end the directive early and inject
/// arbitrary nginx config, and a newline would break out of the YAML block scalar carrying the
/// snippet — which admits no escape sequence, so dropping is the only option there.
/// Returns an empty string when nothing survives — call sites skip the entry.
pub struct NginxHeaderNameEscapeFilter {}

impl NginxHeaderNameEscapeFilter {
    const EXTRA_TOKEN_CHARS: &'static str = "-_.!%&*+^|~";

    fn escape_chars(s: &str) -> String {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric() || Self::EXTRA_TOKEN_CHARS.contains(*c))
            .collect()
    }
}

impl<'a> TeraFilter<'a> for NginxHeaderNameEscapeFilter {
    fn name() -> &'a str {
        "nginx_header_name_escape"
    }

    fn implementation() -> fn(&Value, &HashMap<String, Value>) -> Result<Value, Error> {
        |value: &Value, _: &HashMap<String, Value>| -> Result<Value, Error> {
            let s = try_get_value!("nginx_header_name_escape", "value", String, value);
            Ok(Value::String(NginxHeaderNameEscapeFilter::escape_chars(&s)))
        }
    }
}

/// Escapes an HTTP header value for interpolation inside a double-quoted nginx string.
/// Control chars are dropped: HTTP forbids them in header values, and a newline would
/// both split the response and break out of the YAML block scalar carrying the snippet.
/// `{{` is broken up for the same reason as in [`YamlEncodeFilter`], but with a space
/// rather than an escape sequence, which a block scalar cannot carry.
pub struct NginxHeaderValueEscapeFilter {}

impl NginxHeaderValueEscapeFilter {
    fn escape_chars(s: &str) -> String {
        s.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .replace('\\', "\\\\")
            .replace('\"', "\\\"")
            .replace('\'', "\\'")
            .replace("{{", "{ {")
    }
}

impl<'a> TeraFilter<'a> for NginxHeaderValueEscapeFilter {
    fn name() -> &'a str {
        "nginx_header_value_escape"
    }

    fn implementation() -> fn(&Value, &HashMap<String, Value>) -> Result<Value, Error> {
        |value: &Value, _: &HashMap<String, Value>| -> Result<Value, Error> {
            let s = try_get_value!("nginx_header_value_escape", "value", String, value);
            Ok(Value::String(NginxHeaderValueEscapeFilter::escape_chars(&s)))
        }
    }
}

/// Renders a single template with every filter registered. Chart templates escape user input
/// through those filters, so callers must use this rather than `Tera::one_off`, which knows
/// none of them and fails with `FilterNotFound`.
pub fn render_one_off(template: &str, context: &Context) -> Result<String, Error> {
    let mut tera = Tera::default();
    tera.add_raw_template("one_off", template)?;
    register_filters(&mut tera);
    tera.render("one_off", context)
}

/// Registers every custom filter on a Tera instance. Templates are escaped through these
/// filters, so anything rendering a chart template — production or test — must call this.
pub fn register_filters(tera: &mut Tera) {
    tera.register_filter(Base64EncodeFilter::name(), Base64EncodeFilter::implementation());
    tera.register_filter(YamlEncodeFilter::name(), YamlEncodeFilter::implementation());
    tera.register_filter(HclStringEscapeFilter::name(), HclStringEscapeFilter::implementation());
    tera.register_filter(HclHeredocEscapeFilter::name(), HclHeredocEscapeFilter::implementation());
    tera.register_filter(
        NginxHeaderNameEscapeFilter::name(),
        NginxHeaderNameEscapeFilter::implementation(),
    );
    tera.register_filter(
        NginxHeaderValueEscapeFilter::name(),
        NginxHeaderValueEscapeFilter::implementation(),
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::value::to_value;
    use tera::{Context, Tera};

    use super::*;

    #[test]
    fn test_base64_encode_filter() {
        // setup:
        let test_cases = vec!["", "abc", " abc ", "/jkhbsveir.%"];

        for tc in test_cases {
            // execute:
            let result = Base64EncodeFilter::implementation()(&to_value(tc).unwrap(), &HashMap::new());

            // verify:
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), to_value(general_purpose::STANDARD.encode(tc)).unwrap());
        }
    }

    #[test]
    fn test_base64_encode_filter_injection() {
        // setup:
        const TEST_STR: &str = "abc";

        let mut tera = Tera::default();
        tera.add_raw_template("test", "{{ input | base64_encode }}")
            .expect("Failed to add Tera raw template");
        tera.register_filter(Base64EncodeFilter::name(), Base64EncodeFilter::implementation());

        let mut context = Context::new();
        context.insert("input", TEST_STR);

        // execute:
        let result = tera.render("test", &context).expect("Failed to render Tera template");

        // verify:
        assert_eq!(Base64EncodeFilter::base64_encode(TEST_STR), result);
    }

    #[test]
    fn test_hcl_string_escape_filter() {
        let cases = vec![
            ("plain", "plain"),
            ("with \"quote\"", "with \\\"quote\\\""),
            ("back\\slash", "back\\\\slash"),
            ("${var}", "$${var}"),
            ("%{if x}y%{endif}", "%%{if x}y%%{endif}"),
            ("line1\nline2", "line1\\nline2"),
            ("tab\there", "tab\\there"),
            ("crlf\r\n", "crlf\\r\\n"),
            // Backslash escape runs first, so a quote in the input becomes \" without
            // the inserted backslash being double-escaped.
            ("a\"b", "a\\\"b"),
            // Literal backslash followed by ${ must remain literal: \${var} → \\$${var}
            ("\\${x}", "\\\\$${x}"),
        ];

        for (input, expected) in cases {
            let result = HclStringEscapeFilter::implementation()(&to_value(input).unwrap(), &HashMap::new()).unwrap();
            assert_eq!(result, to_value(expected).unwrap(), "input: {input:?}");
        }
    }

    #[test]
    fn test_hcl_string_escape_filter_in_template() {
        let mut tera = Tera::default();
        tera.add_raw_template("test", r#"value = "{{ input | hcl_string }}""#)
            .unwrap();
        tera.register_filter(HclStringEscapeFilter::name(), HclStringEscapeFilter::implementation());

        let mut context = Context::new();
        context.insert("input", r#"breaks "and" ${interp}"#);

        let result = tera.render("test", &context).unwrap();
        assert_eq!(result, r#"value = "breaks \"and\" $${interp}""#);
    }

    #[test]
    fn test_hcl_heredoc_escape_filter() {
        let cases = vec![
            ("plain text\nwith newlines", "plain text\nwith newlines"),
            ("has ${var}", "has $${var}"),
            ("has %{if x}", "has %%{if x}"),
            // Quotes and backslashes are literal in heredocs — no escaping.
            ("with \"quote\" and \\slash", "with \"quote\" and \\slash"),
        ];

        for (input, expected) in cases {
            let result = HclHeredocEscapeFilter::implementation()(&to_value(input).unwrap(), &HashMap::new()).unwrap();
            assert_eq!(result, to_value(expected).unwrap(), "input: {input:?}");
        }
    }

    /// Breaks out of a double-quoted YAML scalar and adds a sibling field, per QOV-2099.
    const YAML_BREAK_OUT_PAYLOAD: &str = "x\"\n      hostPID: true\n      dummy: \"y";

    fn render_with_filters(template: &str, context: &Context) -> String {
        let mut tera = Tera::default();
        tera.add_raw_template("test", template)
            .expect("Failed to add Tera raw template");
        register_filters(&mut tera);
        tera.render("test", context).expect("Failed to render Tera template")
    }

    #[test]
    fn test_yaml_encode_filter() {
        let cases = vec![
            (to_value("plain").unwrap(), r#""plain""#),
            (to_value("with \"quote\"").unwrap(), r#""with \"quote\"""#),
            (to_value("line1\nline2").unwrap(), r#""line1\nline2""#),
            (to_value("tab\there").unwrap(), r#""tab\there""#),
            // strings stay strings: this is the QOV-2099 type coercion fix
            (to_value("true").unwrap(), r#""true""#),
            (to_value("8080").unwrap(), r#""8080""#),
            // non-strings keep their YAML type, compact JSON being valid YAML
            (to_value(8080).unwrap(), "8080"),
            (to_value(true).unwrap(), "true"),
            (
                serde_json::json!({"secretTargetRef": [{"key": "aws-key"}]}),
                r#"{"secretTargetRef":[{"key":"aws-key"}]}"#,
            ),
        ];

        for (input, expected) in cases {
            let result = YamlEncodeFilter::implementation()(&input, &HashMap::new()).unwrap();
            assert_eq!(result, to_value(expected).unwrap(), "input: {input:?}");
        }
    }

    #[test]
    fn test_yaml_encode_filter_neutralizes_helm_template_actions() {
        // Helm renders the manifest as a Go template after Tera, so no `{{` may survive.
        let result = YamlEncodeFilter::implementation()(
            &to_value(r#"{{ lookup "v1" "Secret" "kube-system" "admin" }}"#).unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        let rendered = result.as_str().expect("filter returns a string");

        assert!(!rendered.contains("{{"), "no Go template delimiter may remain: {rendered}");
        assert_eq!(
            serde_yaml::from_str::<String>(rendered).expect("still a valid YAML scalar"),
            r#"{{ lookup "v1" "Secret" "kube-system" "admin" }}"#,
            "the value itself must survive the escaping"
        );
    }

    /// `{{VAR}}` is the interpolation syntax q-core and the console use, so it has to survive a
    /// round trip byte for byte: the escape exists only in the file Helm reads, and the YAML parser
    /// puts the brace back. Anything here that came out different would silently corrupt a prompt,
    /// an annotation or an environment variable.
    #[test]
    fn test_yaml_encode_filter_round_trips_interpolation_syntax() {
        let cases = [
            "{{WEBHOOK_PAYLOAD}}",
            "{{ WEBHOOK_PAYLOAD }}",
            "process the webhook payload: {{WEBHOOK_PAYLOAD}} ",
            "two {{A}} in one {{B}} value",
            // adjacent and nested braces: the rewrite must not leave a usable delimiter behind
            "{{{{VAR}}}}",
            "{{{VAR}}}",
            "{{",
            "}}",
            "{ {VAR} }",
            // Tera and sprig syntax a user might paste; only `{{` starts a Go template action
            "{% if x %}{{ y }}{% endif %}",
            "{{- lookup \"v1\" \"Secret\" -}}",
        ];

        for input in cases {
            let result = YamlEncodeFilter::implementation()(&to_value(input).unwrap(), &HashMap::new()).unwrap();
            let rendered = result.as_str().expect("filter returns a string");

            assert!(
                !rendered.contains("{{"),
                "input {input:?} left a Go template delimiter: {rendered}"
            );
            assert_eq!(
                serde_yaml::from_str::<String>(rendered).expect("valid YAML scalar"),
                input,
                "input {input:?} did not survive the round trip"
            );
        }
    }

    #[test]
    fn test_yaml_encode_filter_neutralizes_helm_actions_inside_a_non_string_value() {
        // A non-string comes out as a flow collection rather than a quoted scalar, so the escape
        // has to survive there too — it does, because a `{{` can only reach the output from inside
        // a nested quoted scalar.
        let value = serde_json::json!({ "cmd": r#"{{ lookup "v1" "Secret" "kube-system" "admin" }}"#,
                                        "retries": 3, "enabled": true });
        let result = YamlEncodeFilter::implementation()(&value, &HashMap::new()).unwrap();
        let rendered = result.as_str().expect("filter returns a string");

        assert!(!rendered.contains("{{"), "no Go template delimiter may remain: {rendered}");

        let parsed: serde_yaml::Value = serde_yaml::from_str(rendered).expect("still valid YAML");
        assert_eq!(
            parsed["cmd"].as_str(),
            Some(r#"{{ lookup "v1" "Secret" "kube-system" "admin" }}"#),
            "the nested value must survive the escaping"
        );
        assert_eq!(parsed["retries"].as_i64(), Some(3), "numbers keep their type");
        assert_eq!(parsed["enabled"].as_bool(), Some(true), "booleans keep their type");
    }

    #[test]
    fn test_yaml_encode_filter_blocks_node_affinity_break_out() {
        // Mirrors the nodeAffinity block of q-container/templates/deployment.j2.yaml.
        let template = r#"spec:
  template:
    spec:
      affinity:
        nodeAffinity:
          requiredDuringSchedulingIgnoredDuringExecution:
            nodeSelectorTerms:
            - matchExpressions:
              - key: {{ key | yaml_encode }}
                operator: In
                values:
                - {{ value | yaml_encode }}
      containers: []
"#;
        let mut context = Context::new();
        context.insert("key", "topology.kubernetes.io/zone");
        context.insert("value", YAML_BREAK_OUT_PAYLOAD);

        let rendered = render_with_filters(template, &context);
        let manifest: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered manifest must parse");
        let pod_spec = &manifest["spec"]["template"]["spec"];

        assert!(pod_spec["hostPID"].is_null(), "injected hostPID: {rendered}");
        assert!(pod_spec["dummy"].is_null(), "injected dummy field: {rendered}");
        assert_eq!(
            pod_spec["affinity"]["nodeAffinity"]["requiredDuringSchedulingIgnoredDuringExecution"]["nodeSelectorTerms"]
                [0]["matchExpressions"][0]["values"][0]
                .as_str(),
            Some(YAML_BREAK_OUT_PAYLOAD),
            "the payload must land intact inside the node it was meant for"
        );
    }

    #[test]
    fn test_yaml_encode_filter_blocks_mapping_key_break_out() {
        let template = r#"metadata:
  labels:
    {{ key | yaml_encode }}: {{ value | yaml_encode }}
  name: my-service
"#;
        let mut context = Context::new();
        context.insert("key", "evil\n    injected: \"true\"\n    other");
        context.insert("value", "v");

        let rendered = render_with_filters(template, &context);
        let manifest: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered manifest must parse");
        let labels = manifest["metadata"]["labels"]
            .as_mapping()
            .expect("labels must stay a mapping");

        assert_eq!(labels.len(), 1, "a key may only ever produce one entry: {rendered}");
        assert!(
            manifest["metadata"]["labels"]["injected"].is_null(),
            "injected sibling key: {rendered}"
        );
        assert_eq!(manifest["metadata"]["name"].as_str(), Some("my-service"));
    }

    #[test]
    fn test_nginx_header_name_escape_filter() {
        let cases = vec![
            ("X-Forwarded-For", "X-Forwarded-For"),
            ("sec_ch_ua", "sec_ch_ua"),
            // legal token characters must survive rather than be silently mangled
            ("X-Api.Version", "X-Api.Version"),
            ("X-Foo*Bar+Baz~Qux", "X-Foo*Bar+Baz~Qux"),
            // ends the directive and injects nginx config
            ("X-Foo\"; proxy_pass http://evil;", "X-Fooproxy_passhttpevil"),
            // breaks out of the YAML block scalar carrying the snippet
            ("X-Foo\ninjected: true", "X-Fooinjectedtrue"),
            // `#` would comment out the rest of the directive, `$` interpolates
            ("X-Foo#$host", "X-Foohost"),
            ("{{ lookup }}", "lookup"),
            ("", ""),
            ("\n", ""),
        ];

        for (input, expected) in cases {
            let result =
                NginxHeaderNameEscapeFilter::implementation()(&to_value(input).unwrap(), &HashMap::new()).unwrap();
            assert_eq!(result, to_value(expected).unwrap(), "input: {input:?}");
        }
    }

    #[test]
    fn test_nginx_header_value_escape_filter() {
        // setup:
        let mut input_with_expected = HashMap::new();
        input_with_expected.insert("no escape needed", "no escape needed");
        input_with_expected.insert("\"", "\\\"");
        input_with_expected.insert("\\", "\\\\");
        input_with_expected.insert("'", "\\'");
        // control chars are dropped: HTTP forbids them, and a newline escapes the
        // YAML block scalar holding the nginx snippet
        input_with_expected.insert("a\nb", "ab");
        input_with_expected.insert("a\r\nb", "ab");
        input_with_expected.insert("v\";\ninjected: true", "v\\\";injected: true");
        // Helm would evaluate a Go template action left in the snippet
        input_with_expected.insert("{{ lookup }}", "{ { lookup }}");

        for (input, expected) in input_with_expected {
            // execute:
            let result = NginxHeaderValueEscapeFilter::implementation()(&to_value(input).unwrap(), &HashMap::new());

            // verify:
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), to_value(expected).unwrap());
        }
    }

    #[test]
    fn test_nginx_header_value_escape_filter_injection() {
        // setup:
        const INPUT: &str = "some value to escape \\ \" '";
        const EXPECTED: &str = "some value to escape \\\\ \\\" \\'";

        let mut tera = Tera::default();
        tera.add_raw_template("test", "{{ input | nginx_header_value_escape }}")
            .expect("Failed to add Tera raw template");
        tera.register_filter(
            NginxHeaderValueEscapeFilter::name(),
            NginxHeaderValueEscapeFilter::implementation(),
        );

        let mut context = Context::new();
        context.insert("input", INPUT);

        // execute:
        let result = tera.render("test", &context).expect("Failed to render Tera template");

        // verify:
        assert_eq!(result, EXPECTED);
    }
}
