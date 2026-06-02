use base64::Engine;
use base64::engine::general_purpose;
use serde_json::Value;
use std::collections::HashMap;
use tera::{Error, try_get_value};

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

/// Encodes string value to base 64.
pub struct NginxHeaderValueEscapeFilter {}

impl NginxHeaderValueEscapeFilter {
    fn escape_chars(s: &str) -> String {
        s.replace('\\', "\\\\").replace('\"', "\\\"").replace('\'', "\\'")
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

    #[test]
    fn test_nginx_header_value_escape_filter() {
        // setup:
        let mut input_with_expected = HashMap::new();
        input_with_expected.insert("no escape needed", "no escape needed");
        input_with_expected.insert("\"", "\\\"");
        input_with_expected.insert("\\", "\\\\");
        input_with_expected.insert("'", "\\'");

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
