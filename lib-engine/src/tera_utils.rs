use serde_json::Value;
use std::collections::HashMap;
use tera::{try_get_value, Error};

/// This file to declare custom functions / filters and stuff for tera
/// documentation => https://keats.github.io/tera/docs/#advanced-usage

// TODO(benjaminch): this should be an external crate

pub trait TeraFilter<'a> {
    fn name() -> &'a str;
    fn implementation() -> fn(&Value, &HashMap<String, Value>) -> Result<Value, Error>;
}

/// Encodes string value to base 64.
pub struct Base64EncodeFilter {}

impl Base64EncodeFilter {
    fn base64_encode(s: &str) -> String {
        base64::encode(s)
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
            assert_eq!(result.unwrap(), to_value(base64::encode(tc)).unwrap());
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
