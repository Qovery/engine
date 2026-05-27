use crate::helm::HelmChartError;
use base64::Engine;
use serde_json::Value;

/// Validate an Envoy access log JSON format and base64 encode it for Helm transport.
pub(crate) fn encode_envoy_access_log_format(chart_name: &str, format: &str) -> Result<String, HelmChartError> {
    // Strip surrounding quotes if present (some configs store it as "\"{ ... }\"").
    let mut unquoted = serde_json::from_str::<String>(format).unwrap_or_else(|_| format.to_string());

    // If still wrapped in quotes, strip repeatedly to handle double-escaping.
    while unquoted.len() > 2 && unquoted.starts_with('"') && unquoted.ends_with('"') {
        unquoted = unquoted[1..unquoted.len() - 1].to_string();
    }

    // Normalize line breaks and tabs before JSON parsing.
    let normalized: String = unquoted
        .chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            _ => c,
        })
        .collect();

    let json_value = serde_json::from_str::<Value>(&normalized).map_err(|e| HelmChartError::RenderingError {
        chart_name: chart_name.to_string(),
        msg: format!("Invalid JSON format for envoy access log format: {}", e),
    })?;

    let minified = serde_json::to_string(&json_value).map_err(|e| HelmChartError::RenderingError {
        chart_name: chart_name.to_string(),
        msg: format!("Failed to serialize envoy access log format: {}", e),
    })?;

    Ok(base64::engine::general_purpose::STANDARD.encode(minified))
}
