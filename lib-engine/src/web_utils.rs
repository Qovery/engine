pub fn is_valid_http_token(value: &str) -> bool {
    // RFC 9110 token chars: !#$%&'*+-.^_`|~ plus alnum.
    !value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub fn validate_http_header_name(name: &str) -> Result<(), String> {
    if is_valid_http_token(name) {
        Ok(())
    } else {
        Err(format!(
            "invalid sticky session header name '{name}'. Header names must be a valid HTTP token (letters, digits, and !#$%&'*+-.^_`|~)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{is_valid_http_token, validate_http_header_name};

    #[test]
    fn http_token_validation_accepts_valid_values() {
        assert!(is_valid_http_token("X-Header"));
        assert!(is_valid_http_token("Mcp-Session-Id"));
        assert!(is_valid_http_token("x_foo.bar~baz"));
    }

    #[test]
    fn http_token_validation_rejects_invalid_values() {
        assert!(!is_valid_http_token(""));
        assert!(!is_valid_http_token("X Header"));
        assert!(!is_valid_http_token("X:Header"));
    }

    #[test]
    fn validate_header_name_returns_error_on_invalid_input() {
        assert!(validate_http_header_name("X Header").is_err());
    }
}
