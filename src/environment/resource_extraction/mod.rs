// Resource extraction module for terraform resources
// This module extracts and parses terraform resources from state files
pub use parser::TerraformResource;

pub mod parser;
pub mod sensitive_filter;
