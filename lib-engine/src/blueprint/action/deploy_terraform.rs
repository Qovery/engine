use crate::blueprint::action::render_and_apply;
use crate::blueprint::models::error::BlueprintError;
use crate::blueprint::models::info::BlueprintInfo;
use crate::blueprint::models::qovery_blueprint_manifest::CredentialMode;
use crate::blueprint::models::spec::{ResolvedBackend, ResolvedTerraformSpec, TemplateVariable, TerraformFlavor};
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::io_models::blueprint::BlueprintRequest;
use crate::logger::Logger;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

// TODO: Should come from the QBM spec (blueprint author pins terraform version).
const DEFAULT_ENGINE_VERSION: &str = "1.9.7";

/// Tera context for the blueprint terraform template.
#[derive(Serialize)]
struct BlueprintTerraformTeraContext {
    environment_id: String,
    name: String,
    git_url: String,
    git_branch: String,
    git_root_path: String,
    git_token_id: Option<String>,
    engine: String,
    engine_version: String,
    timeout_seconds: u64,
    use_cluster_credentials: bool,
    backend_kubernetes: bool,
    backend_blueprint: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    backend_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backend_config: Option<HashMap<String, String>>,
    job_cpu_milli: u32,
    job_ram_mib: u32,
    job_storage_gib: u32,
    variables: Vec<TemplateVariable>,
    import_id: Option<String>,
}

/// Execute a Terraform blueprint: build Tera context → render template → terraform init + apply.
pub fn execute(
    lib_root_dir: &str,
    spec: &ResolvedTerraformSpec,
    request: &BlueprintRequest,
    blueprint_info: &BlueprintInfo,
    is_dry_run: bool,
    event_details: &EventDetails,
    logger: &dyn Logger,
) -> Result<(), Box<EngineError>> {
    let template_dir = PathBuf::from(lib_root_dir).join("blueprint").join("terraform");
    let ctx = tera::Context::from_serialize(BlueprintTerraformTeraContext::new(spec, request, blueprint_info))
        .map_err(|e| {
            Box::new(EngineError::new_blueprint_error(
                event_details.clone(),
                BlueprintError::TerraformGenerationError(format!("Failed to build Tera context: {}", e)),
            ))
        })?;

    render_and_apply(
        &template_dir,
        &ctx,
        &request.qovery_api_token,
        spec.timeout_sec,
        is_dry_run,
        "qovery_terraform_service",
        event_details,
        logger,
    )
}

impl BlueprintTerraformTeraContext {
    fn new(spec: &ResolvedTerraformSpec, request: &BlueprintRequest, blueprint_info: &BlueprintInfo) -> Self {
        let (backend_type, backend_config) = match &spec.backend {
            ResolvedBackend::Blueprint { backend_type, config } => (Some(backend_type.clone()), Some(config.clone())),
            _ => (None, None),
        };

        Self {
            environment_id: request.environment_id.clone(),
            name: request.name.clone(),
            git_url: request.git_url.clone(),
            git_branch: request.tag.clone(),
            git_root_path: blueprint_info.path(),
            git_token_id: request.git_credentials.as_ref().map(|c| c.login.clone()),
            engine: match spec.flavor {
                TerraformFlavor::Terraform => "TERRAFORM".to_string(),
                TerraformFlavor::OpenTofu => "OPEN_TOFU".to_string(),
            },
            engine_version: DEFAULT_ENGINE_VERSION.to_string(),
            timeout_seconds: spec.timeout_sec,
            use_cluster_credentials: matches!(spec.credential_mode, CredentialMode::Cluster),
            backend_kubernetes: matches!(spec.backend, ResolvedBackend::Qovery),
            backend_blueprint: matches!(spec.backend, ResolvedBackend::Blueprint { .. }),
            backend_type,
            backend_config,
            job_cpu_milli: spec.job_resources.cpu_milli,
            job_ram_mib: spec.job_resources.ram_mib,
            job_storage_gib: spec.job_resources.storage_gib,
            variables: request
                .variables
                .iter()
                .map(|v| TemplateVariable {
                    name: v.name.clone(),
                    value: v.value.clone(),
                    is_secret: v.is_secret,
                })
                .collect(),
            import_id: request.import_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::models::spec::{JobResources, ResolvedBackend, ResolvedTerraformSpec, TerraformFlavor};
    use crate::io_models::blueprint::{BlueprintRequest, BlueprintVariable};
    use crate::template::generate_and_copy_all_files_into_dir;

    fn test_request() -> BlueprintRequest {
        BlueprintRequest {
            execution_id: "exec-1".into(),
            long_id: uuid::Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap(),
            name: "my-s3-bucket".into(),
            kube_name: "my-s3-bucket".into(),
            project_long_id: uuid::Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap(),
            organization_long_id: uuid::Uuid::parse_str("22222222-3333-4444-5555-666666666666").unwrap(),
            max_parallel_build: 1,
            max_parallel_deploy: 1,
            variables: vec![
                BlueprintVariable {
                    name: "region".into(),
                    value: "eu-west-3".into(),
                    is_secret: false,
                },
                BlueprintVariable {
                    name: "bucket_name".into(),
                    value: "my-prod-bucket".into(),
                    is_secret: false,
                },
            ],
            git_url: "https://github.com/org/catalog.git".into(),
            tag: "aws/s3/1/1.0.0".into(),
            git_credentials: None,
            spec_overrides: None,
            qovery_api_token: "test-token".into(),
            environment_id: "env-uuid".into(),
            import_id: None,
        }
    }

    fn test_spec() -> ResolvedTerraformSpec {
        ResolvedTerraformSpec {
            flavor: TerraformFlavor::Terraform,
            provider: "aws".into(),
            credential_mode: CredentialMode::Cluster,
            backend: ResolvedBackend::Qovery,
            timeout_sec: 1800,
            outputs: vec![],
            job_resources: JobResources {
                cpu_milli: 500,
                ram_mib: 512,
                storage_gib: 20,
            },
        }
    }

    fn test_info() -> BlueprintInfo {
        BlueprintInfo::try_new("aws/s3/1/1.0.0").unwrap()
    }

    fn render_template(spec: &ResolvedTerraformSpec, request: &BlueprintRequest, info: &BlueprintInfo) -> String {
        let ctx = tera::Context::from_serialize(BlueprintTerraformTeraContext::new(spec, request, info)).unwrap();
        let template_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lib/blueprint/terraform");
        let temp_dir = tempfile::TempDir::new().unwrap();
        generate_and_copy_all_files_into_dir(&template_dir, temp_dir.path(), &ctx).unwrap();
        std::fs::read_to_string(temp_dir.path().join("main.tf")).unwrap()
    }

    #[test]
    fn generate_main_tf_renders_all_fields() {
        let result = render_template(&test_spec(), &test_request(), &test_info());

        assert!(result.contains(r#"source = "qovery/qovery""#));
        assert!(result.contains(r#"provider "qovery" {}"#));
        assert!(result.contains(r#"environment_id        = "env-uuid""#));
        assert!(result.contains(r#"name                  = "my-s3-bucket""#));
        assert!(result.contains("auto_deploy           = true"));
        assert!(result.contains(r#"engine                = "TERRAFORM""#));
        assert!(result.contains("timeout_seconds       = 1800"));
        assert!(result.contains("use_cluster_credentials = true"));
        assert!(result.contains(r#"url       = "https://github.com/org/catalog.git""#));
        assert!(result.contains(r#"branch    = "aws/s3/1/1.0.0""#));
        assert!(result.contains(r#"root_path = "aws/s3/1""#));
        assert!(result.contains("backend = {"));
        assert!(result.contains("kubernetes = {}"));
        assert!(result.contains("git_repository = {"));
        assert!(result.contains("engine_version = {"));
        assert!(result.contains("job_resources = {"));
        assert!(result.contains("cpu_milli   = 500"));
        assert!(result.contains("ram_mib     = 512"));
        assert!(result.contains("storage_gib = 20"));
        assert!(result.contains(r#"key       = "TF_VAR_region""#));
        assert!(result.contains("tfvars_files = []"));
        assert!(!result.contains("import {"));
    }

    #[test]
    fn generate_main_tf_with_import() {
        let mut request = test_request();
        request.import_id = Some("service-uuid-123".into());
        let result = render_template(&test_spec(), &request, &test_info());

        assert!(result.contains("import {"));
        assert!(result.contains("to = qovery_terraform_service.blueprint"));
        assert!(result.contains(r#"id = "service-uuid-123""#));
    }

    #[test]
    fn generate_main_tf_blueprint_backend() {
        let mut spec = test_spec();
        let mut config = HashMap::new();
        config.insert("bucket".to_string(), "my-state-bucket".to_string());
        config.insert("region".to_string(), "eu-west-3".to_string());
        spec.backend = ResolvedBackend::Blueprint {
            backend_type: "s3".into(),
            config,
        };
        let result = render_template(&spec, &test_request(), &test_info());

        assert!(result.contains("blueprint = {"));
        assert!(result.contains(r#"type = "s3""#));
        assert!(result.contains(r#"bucket = "my-state-bucket""#));
        assert!(!result.contains("kubernetes = {}"));
    }

    #[test]
    fn generate_main_tf_opentofu_env_credentials() {
        let mut spec = test_spec();
        spec.flavor = TerraformFlavor::OpenTofu;
        spec.credential_mode = CredentialMode::Env;
        let result = render_template(&spec, &test_request(), &test_info());

        assert!(result.contains(r#"engine                = "OPEN_TOFU""#));
        assert!(result.contains("use_cluster_credentials = false"));
    }

    #[test]
    fn generate_main_tf_secret_variable() {
        let mut request = test_request();
        request.variables = vec![BlueprintVariable {
            name: "db_password".into(),
            value: "s3cret!".into(),
            is_secret: true,
        }];
        let result = render_template(&test_spec(), &request, &test_info());

        assert!(result.contains(r#"key       = "TF_VAR_db_password""#));
        assert!(result.contains("is_secret = true"));
    }
}
