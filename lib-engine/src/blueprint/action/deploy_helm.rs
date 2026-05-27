use crate::blueprint::action::render_and_apply;
use crate::blueprint::models::error::BlueprintError;
use crate::blueprint::models::info::BlueprintInfo;
use crate::blueprint::models::spec::ResolvedHelmSpec;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::io_models::blueprint::{BlueprintRequest, BlueprintVariable};
use crate::logger::Logger;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Tera context for the blueprint helm template.
#[derive(Serialize)]
struct BlueprintHelmTeraContext {
    organization_id: String,
    environment_id: String,
    name: String,
    execution_id: String,
    service_name: String,
    chart_repository: String,
    chart_name: String,
    chart_version: String,
    allow_cluster_wide_resources: bool,
    timeout_sec: u64,
    arguments: Vec<String>,
    rendered_values: Option<String>,
    import_id: Option<String>,
}

/// Execute a Helm blueprint: render values.yaml → build Tera context → render template → terraform init + apply.
pub fn execute(
    working_dir: &Path,
    lib_root_dir: &str,
    spec: &ResolvedHelmSpec,
    request: &BlueprintRequest,
    blueprint_info: &BlueprintInfo,
    is_dry_run: bool,
    event_details: &EventDetails,
    logger: &dyn Logger,
) -> Result<(), Box<EngineError>> {
    // 1. Render values.yaml if present
    let rendered_values = render_values_yaml(working_dir, &request.variables).map_err(|e| {
        Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::TerraformGenerationError(format!("Failed to render values.yaml: {}", e)),
        ))
    })?;

    // 2. Build context
    let template_dir = PathBuf::from(lib_root_dir).join("blueprint").join("helm");
    let ctx =
        tera::Context::from_serialize(BlueprintHelmTeraContext::new(spec, request, blueprint_info, rendered_values))
            .map_err(|e| {
                Box::new(EngineError::new_blueprint_error(
                    event_details.clone(),
                    BlueprintError::TerraformGenerationError(format!("Failed to build Tera context: {}", e)),
                ))
            })?;

    // 3. Render
    render_and_apply(
        &template_dir,
        &ctx,
        &request.qovery_api_token,
        spec.timeout_sec,
        is_dry_run,
        "qovery_helm",
        event_details,
        logger,
    )
}

/// Render the blueprint's values.yaml template with Tera, replacing {{ var }} placeholders.
/// Returns None if no values.yaml exists.
fn render_values_yaml(blueprint_dir: &Path, variables: &[BlueprintVariable]) -> Result<Option<String>, String> {
    let values_path = blueprint_dir.join("values.yaml");
    if !values_path.exists() {
        return Ok(None);
    }

    let template = std::fs::read_to_string(&values_path).map_err(|e| format!("Failed to read values.yaml: {}", e))?;

    let mut tera = tera::Tera::default();
    tera.add_raw_template("values.yaml", &template)
        .map_err(|e| format!("Failed to parse values.yaml template: {}", e))?;

    let mut ctx = tera::Context::new();
    for var in variables {
        ctx.insert(&var.name, &var.value);
    }

    tera.render("values.yaml", &ctx)
        .map_err(|e| format!("Failed to render values.yaml: {}", e))
        .map(Some)
}

impl BlueprintHelmTeraContext {
    fn new(
        spec: &ResolvedHelmSpec,
        request: &BlueprintRequest,
        blueprint_info: &BlueprintInfo,
        rendered_values: Option<String>,
    ) -> Self {
        Self {
            organization_id: request.organization_long_id.to_string(),
            environment_id: request.environment_id.clone(),
            name: request.name.clone(),
            execution_id: request.execution_id.clone(),
            service_name: blueprint_info.service_name().to_string(),
            chart_repository: spec.chart.repository.clone(),
            chart_name: spec.chart.name.clone(),
            chart_version: spec.chart.version.clone(),
            allow_cluster_wide_resources: spec.allow_cluster_wide_resources,
            timeout_sec: spec.timeout_sec,
            arguments: spec.arguments.clone(),
            rendered_values,
            import_id: request.import_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::models::qovery_blueprint_manifest::{BlueprintChart, CredentialMode};
    use crate::io_models::blueprint::BlueprintVariable;
    use crate::template::generate_and_copy_all_files_into_dir;
    use std::fs;
    use tempfile::TempDir;

    fn test_request() -> BlueprintRequest {
        BlueprintRequest {
            execution_id: "exec-1".into(),
            long_id: uuid::Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap(),
            name: "my-redis".into(),
            kube_name: "my-redis".into(),
            project_long_id: uuid::Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap(),
            organization_long_id: uuid::Uuid::parse_str("22222222-3333-4444-5555-666666666666").unwrap(),
            max_parallel_build: 1,
            max_parallel_deploy: 1,
            variables: vec![
                BlueprintVariable {
                    name: "password".into(),
                    value: "s3cret".into(),
                    is_secret: true,
                },
                BlueprintVariable {
                    name: "memory_limit".into(),
                    value: "256Mi".into(),
                    is_secret: false,
                },
            ],
            git_url: "https://github.com/org/catalog.git".into(),
            tag: "helm/redis/7/1.0.0".into(),
            git_credentials: None,
            spec_overrides: None,
            qovery_api_token: "test-token".into(),
            environment_id: "env-uuid".into(),
            import_id: None,
        }
    }

    fn test_spec() -> ResolvedHelmSpec {
        ResolvedHelmSpec {
            chart: BlueprintChart {
                repository: "https://charts.bitnami.com/bitnami".into(),
                name: "redis".into(),
                version: "25.3.11".into(),
            },
            credential_mode: CredentialMode::Cluster,
            timeout_sec: 600,
            arguments: vec!["--atomic".into()],
            allow_cluster_wide_resources: false,
            outputs: vec![],
        }
    }

    fn test_info() -> BlueprintInfo {
        BlueprintInfo::try_new("helm/redis/7/1.0.0").unwrap()
    }

    fn render_template(
        spec: &ResolvedHelmSpec,
        request: &BlueprintRequest,
        info: &BlueprintInfo,
        rendered_values: Option<String>,
    ) -> String {
        let ctx =
            tera::Context::from_serialize(BlueprintHelmTeraContext::new(spec, request, info, rendered_values)).unwrap();
        let template_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lib/blueprint/helm");
        let temp_dir = TempDir::new().unwrap();
        generate_and_copy_all_files_into_dir(&template_dir, temp_dir.path(), &ctx).unwrap();
        fs::read_to_string(temp_dir.path().join("main.tf")).unwrap()
    }

    #[test]
    fn generate_helm_tf_renders_all_fields() {
        let result = render_template(&test_spec(), &test_request(), &test_info(), None);

        assert!(result.contains(r#"source = "qovery/qovery""#));
        assert!(result.contains(r#"resource "qovery_helm_repository" "blueprint_repo""#));
        assert!(result.contains(r#"organization_id       = "22222222-3333-4444-5555-666666666666""#));
        assert!(result.contains(r#"url                   = "https://charts.bitnami.com/bitnami""#));
        assert!(result.contains(r#"resource "qovery_helm" "blueprint""#));
        assert!(result.contains(r#"environment_id               = "env-uuid""#));
        assert!(result.contains(r#"name                         = "my-redis""#));
        assert!(result.contains("allow_cluster_wide_resources = false"));
        assert!(result.contains("auto_deploy                  = true"));
        assert!(result.contains("timeout_sec                  = 600"));
        assert!(result.contains(r#"chart_name         = "redis""#));
        assert!(result.contains(r#"chart_version      = "25.3.11""#));
        assert!(result.contains("helm_repository_id = qovery_helm_repository.blueprint_repo.id"));
        assert!(result.contains("source = {"));
        assert!(result.contains("helm_repository = {"));
        assert!(result.contains("values_override = {"));
        assert!(result.contains(r#""--atomic""#));
        assert!(!result.contains("import {"));
    }

    #[test]
    fn generate_helm_tf_with_rendered_values() {
        let values = "auth:\n  password: \"s3cret\"".to_string();
        let result = render_template(&test_spec(), &test_request(), &test_info(), Some(values));

        assert!(result.contains("blueprint-values"));
        assert!(result.contains("auth:"));
        assert!(result.contains("file = {"));
    }

    #[test]
    fn generate_helm_tf_with_import() {
        let mut request = test_request();
        request.import_id = Some("helm-service-uuid".into());
        let result = render_template(&test_spec(), &request, &test_info(), None);

        assert!(result.contains("import {"));
        assert!(result.contains("to = qovery_helm.blueprint"));
        assert!(result.contains(r#"id = "helm-service-uuid""#));
    }

    #[test]
    fn render_values_yaml_with_variables() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("values.yaml"), "password: \"{{ password }}\"").unwrap();
        let vars = vec![BlueprintVariable {
            name: "password".into(),
            value: "s3cret!".into(),
            is_secret: true,
        }];

        let result = render_values_yaml(dir.path(), &vars).unwrap().unwrap();
        assert!(result.contains("s3cret!"));
        assert!(!result.contains("{{"));
    }

    #[test]
    fn render_values_yaml_no_file_returns_none() {
        let dir = TempDir::new().unwrap();
        assert!(render_values_yaml(dir.path(), &[]).unwrap().is_none());
    }
}
