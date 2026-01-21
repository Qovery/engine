// Terraform resource parser
// Parses terraform show -json output to extract resources

use super::sensitive_filter::SensitiveFilter;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// Domain Model
// ============================================================================

/// Represents a single terraform resource (domain model)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerraformResource {
    pub resource_type: String,
    pub name: String,
    pub provider: String,
    pub address: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_action: Option<String>,
    pub attributes: serde_json::Map<String, serde_json::Value>,
}

// ============================================================================
// Infrastructure DTOs (match terraform show -json schema for 1.3+)
// ============================================================================

/// Root structure of terraform show -json output (terraform 1.3+)
#[derive(Debug, Deserialize)]
struct TerraformShowOutput {
    values: TerraformValues,
}

/// Values section containing root module
#[derive(Debug, Deserialize)]
struct TerraformValues {
    root_module: TerraformModule,
}

/// Terraform module containing resources and child modules
#[derive(Debug, Deserialize)]
struct TerraformModule {
    #[serde(default)]
    resources: Vec<TerraformResourceDto>,
    #[serde(default)]
    child_modules: Vec<TerraformModule>,
}

/// Terraform resource DTO (infrastructure layer)
#[derive(Debug, Deserialize)]
struct TerraformResourceDto {
    #[serde(rename = "type")]
    resource_type: String,
    name: String,
    address: String,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default)]
    values: serde_json::Map<String, Value>,
}

fn default_mode() -> String {
    "managed".to_string()
}

// ============================================================================
// Parser
// ============================================================================

/// Parser for terraform resources from terraform show -json output
pub struct TerraformResourceParser {
    filter: SensitiveFilter,
}

impl TerraformResourceParser {
    pub fn new() -> Self {
        TerraformResourceParser {
            filter: SensitiveFilter::new(),
        }
    }

    /// Parse terraform show -json output from a Value and extract resources (terraform 1.3+)
    pub fn parse_from_value(&self, json_value: Value) -> Result<Vec<TerraformResource>, String> {
        // Deserialize to infrastructure DTO from Value
        let output: TerraformShowOutput =
            serde_json::from_value(json_value).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        // Extract resources from root module (and recursively from child modules)
        Ok(self.extract_from_module(&output.values.root_module))
    }

    /// Parse terraform show -json output and extract resources (terraform 1.3+)
    pub fn parse(&self, json_output: &str) -> Result<Vec<TerraformResource>, String> {
        // Deserialize to infrastructure DTO
        let output: TerraformShowOutput =
            serde_json::from_str(json_output).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        // Extract resources from root module (and recursively from child modules)
        Ok(self.extract_from_module(&output.values.root_module))
    }

    /// Extract resources from a module and its child modules (recursive)
    fn extract_from_module(&self, module: &TerraformModule) -> Vec<TerraformResource> {
        let mut resources: Vec<TerraformResource> =
            module.resources.iter().map(|dto| self.map_to_domain(dto)).collect();

        // Recursively extract from child modules
        for child_module in &module.child_modules {
            resources.extend(self.extract_from_module(child_module));
        }

        resources
    }

    /// Map infrastructure DTO to domain model with sensitive data filtering
    fn map_to_domain(&self, dto: &TerraformResourceDto) -> TerraformResource {
        // Extract provider from resource type (e.g., "aws_instance" -> "aws")
        let provider = dto.resource_type.split('_').next().unwrap_or("unknown").to_string();

        // Filter sensitive attributes
        let mut filtered_attributes = serde_json::Map::new();
        for (key, value) in &dto.values {
            if !self.filter.is_sensitive(key) {
                filtered_attributes.insert(key.clone(), value.clone());
            }
        }

        TerraformResource {
            resource_type: dto.resource_type.clone(),
            name: dto.name.clone(),
            provider,
            address: dto.address.clone(),
            mode: dto.mode.clone(),
            change_action: None,
            attributes: filtered_attributes,
        }
    }
}

impl Default for TerraformResourceParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_json() {
        let parser = TerraformResourceParser::new();
        // Empty JSON is invalid for terraform 1.3+ (requires "values" field)
        assert!(parser.parse("{}").is_err());
    }

    #[test]
    fn test_parse_invalid_json() {
        let parser = TerraformResourceParser::new();
        assert!(parser.parse("not json").is_err());
    }

    #[test]
    fn test_parse_real_terraform_show_format() {
        let parser = TerraformResourceParser::new();
        // Real terraform show -json format (direct values, no state wrapper)
        let json = serde_json::json!({
            "format_version": "1.0",
            "terraform_version": "1.9.7",
            "values": {
                "root_module": {
                    "resources": [
                        {
                            "address": "qovery_container.my_container",
                            "mode": "managed",
                            "type": "qovery_container",
                            "name": "my_container",
                            "provider_name": "registry.terraform.io/qovery/qovery",
                            "values": {
                                "id": "61cc4eaf-5174-4d16-a68e-41055831fe3c",
                                "name": "castlemock_tf",
                                "image_name": "castlemock/castlemock",
                                "tag": "v1.64",
                                "cpu": 500,
                                "memory": 512
                            }
                        },
                        {
                            "address": "qovery_deployment.my_deployment",
                            "mode": "managed",
                            "type": "qovery_deployment",
                            "name": "my_deployment",
                            "provider_name": "registry.terraform.io/qovery/qovery",
                            "values": {
                                "id": "61cc4eaf-5174-4d16-a68e-41055831fe3d",
                                "desired_state": "RUNNING",
                                "environment_id": "61cc4eaf-5174-4d16-a68e-41055831fe3e"
                            }
                        }
                    ]
                }
            }
        });

        let result = parser.parse(&json.to_string());
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());

        let resources = result.unwrap();
        assert_eq!(resources.len(), 2, "Expected 2 resources");

        // Check first resource
        assert_eq!(resources[0].resource_type, "qovery_container");
        assert_eq!(resources[0].name, "my_container");
        assert_eq!(resources[0].provider, "qovery");
        assert_eq!(resources[0].address, "qovery_container.my_container");
        assert_eq!(resources[0].mode, "managed");

        // Check resource has expected attributes
        assert!(resources[0].attributes.contains_key("id"));
        assert!(resources[0].attributes.contains_key("name"));
        assert!(resources[0].attributes.contains_key("cpu"));

        // Check second resource
        assert_eq!(resources[1].resource_type, "qovery_deployment");
        assert_eq!(resources[1].name, "my_deployment");
        assert_eq!(resources[1].provider, "qovery");
    }
}
