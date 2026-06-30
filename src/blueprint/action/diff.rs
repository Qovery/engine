//! Underlying-infra diff for the blueprint DIFF action — terraform-typed blueprints only.
//!
//! For terraform: this module runs the diff on the **catalog's actual terraform module** against
//! the deployed state, so the payload is the real infra change (e.g. `aws_db_instance.db will be
//! updated: instance_class db.t3.micro -> db.t3.small`). Engine renders user variables into
//! `qovery.auto.tfvars`, points the kubernetes backend at the deployed service's tfstate secret,
//! runs `terraform init + plan`. Backend secret name follows the env-engine convention
//! `tfstate-default-{service_id}` (see `crate::infrastructure::models::cloud_provider::service::get_tfstate_name`),
//! which the BlueprintRequest carries as `import_id`.
//!
//! For helm: the DIFF path uses [`super::render_and_diff`] (via
//! `super::deploy_helm::execute_diff`) which produces a qovery-service-level diff — changes to the
//! `qovery_helm` resource fields (chart version pin, rendered values). That's the right
//! granularity for helm: blueprint catalogs only ship `values.yaml` + `qbm.yml`, the chart itself
//! is a pinned reference, so a catalog tag bump's changes are fully expressed by the wrapper.

use crate::blueprint::models::error::BlueprintError;
use crate::cmd::terraform::{TerraformOutput, terraform_init_validate_lock_free, terraform_plan_internal};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EventDetails, EventMessage};
use crate::io_models::blueprint::BlueprintRequest;
use crate::io_models::terraform::TerraformBackendType;
use crate::logger::Logger;
use std::fs;
use std::path::Path;

pub const DIFF_PAYLOAD_MAX_BYTES: usize = 1_048_576; // 1 MiB

/// Underlying-infra diff for a Terraform blueprint: pull the catalog's `*.tf` module, render
/// variables, wire the kubernetes state backend at the deployed service's tfstate secret, run
/// `terraform init + plan`, return the human-readable plan output.
///
pub fn diff_underlying_terraform(
    blueprint_dir: &Path,
    request: &BlueprintRequest,
    cloud_envs: &[(&str, &str)],
    kubeconfig_path: &Path,
    event_details: &EventDetails,
    logger: &dyn Logger,
) -> Result<String, Box<EngineError>> {
    let backend_type = request.backend_type.ok_or_else(|| {
        Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::TerraformGenerationError(
                "DIFF action requires `backend_type` (resolved terraform backend mode of the deployed service) — missing from BlueprintRequest".to_string(),
            ),
        ))
    })?;
    let import_id = request.import_id.as_deref().ok_or_else(|| {
        Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::TerraformGenerationError(
                "DIFF action requires `import_id` (id of the deployed terraform service) — missing from BlueprintRequest".to_string(),
            ),
        ))
    })?;
    // env_kube_name is only consumed by the Kubernetes-backend override path; user-defined
    // backends carry their own location in the catalog's HCL.
    if matches!(backend_type, TerraformBackendType::Kubernetes) && request.env_kube_name.is_empty() {
        return Err(Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::TerraformGenerationError(
                "DIFF action with Kubernetes backend requires `env_kube_name` — missing from BlueprintRequest"
                    .to_string(),
            ),
        )));
    }

    // 1. Copy the catalog module into a writable workspace; tfvars + backend config land alongside.
    //    blueprint_dir is already the per-blueprint subdir (clone_blueprint_repo returns
    //    clone_dir.join(blueprint_info.path())) so we use it directly — no second join.
    let workspace = tempfile::TempDir::new().map_err(|e| {
        Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::WorkspaceError(format!("Failed to create terraform diff workspace: {}", e)),
        ))
    })?;
    copy_dir_contents(blueprint_dir, workspace.path()).map_err(|e| {
        Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::WorkspaceError(format!(
                "Failed to copy catalog module from {} into workspace: {}",
                blueprint_dir.display(),
                e
            )),
        ))
    })?;

    // 2. Render user variables as `qovery.auto.tfvars` — terraform picks up `*.auto.tfvars` automatically.
    let tfvars_path = workspace.path().join("qovery.auto.tfvars");
    fs::write(&tfvars_path, render_tfvars(request)).map_err(|e| {
        Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::TerraformGenerationError(format!("Failed to write qovery.auto.tfvars: {}", e)),
        ))
    })?;

    // 3. If the deployed service uses Qovery's k8s backend, override the catalog's declaration
    //    with one pointed at the right tfstate Secret. For user-defined backends, the catalog's
    //    own `backend { … }` block is already correct — emit no override.
    match backend_type {
        TerraformBackendType::Kubernetes => {
            // see get_tfstate_name
            let tfstate_secret_name = format!("tfstate-default-{}", import_id);
            let backend_path = workspace.path().join("zz_qovery_backend_override.tf");
            fs::write(
                &backend_path,
                render_kubernetes_backend(&tfstate_secret_name, &request.env_kube_name),
            )
            .map_err(|e| {
                Box::new(EngineError::new_blueprint_error(
                    event_details.clone(),
                    BlueprintError::TerraformGenerationError(format!("Failed to write backend override: {}", e)),
                ))
            })?;
            logger.log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new(
                    format!(
                        "Diffing terraform blueprint against Qovery-managed tfstate secret {} in namespace {}",
                        tfstate_secret_name, request.env_kube_name
                    ),
                    None,
                ),
            ));
        }
        TerraformBackendType::DefinedInTerraformFile => {
            logger.log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new(
                    "Diffing terraform blueprint against the catalog's user-defined backend (no override emitted)"
                        .to_string(),
                    None,
                ),
            ));
        }
    }

    // 4. Compose terraform's env: cloud-provider creds + kubeconfig so the kubernetes backend (when
    //    used) can read the deployed tfstate Secret. The terraform kubernetes backend reads
    //    KUBE_CONFIG_PATH (not KUBECONFIG), so that's the one that matters here.
    let kubeconfig_str = kubeconfig_path.to_string_lossy().into_owned();
    let mut envs: Vec<(&str, &str)> = cloud_envs.to_vec();
    envs.push(("KUBE_CONFIG_PATH", &kubeconfig_str));

    let dir = workspace.path().to_string_lossy();

    logger.log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new("Running terraform init + validate on underlying module".to_string(), None),
    ));
    terraform_init_validate_lock_free(&dir, &envs, &TerraformValidators::Default)
        .map_err(|e| Box::new(EngineError::new_terraform_error(event_details.clone(), e)))?;

    logger.log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new("Running terraform plan on underlying module".to_string(), None),
    ));
    // Preview is read-only — disable state locking so it never blocks a real deploy
    let plan_output = terraform_plan_internal(&dir, &envs, &TerraformValidators::Default, false, false)
        .map_err(|e| Box::new(EngineError::new_terraform_error(event_details.clone(), e)))?;

    Ok(truncate_diff_payload(&plan_output))
}

/// Joins captured `terraform plan` stdout into a single string. Truncates with an elision
/// marker if the result exceeds [DIFF_PAYLOAD_MAX_BYTES].
pub(crate) fn truncate_diff_payload(output: &TerraformOutput) -> String {
    let raw = output.raw_std_output.join("\n");
    if raw.len() <= DIFF_PAYLOAD_MAX_BYTES {
        return raw;
    }
    let half = DIFF_PAYLOAD_MAX_BYTES / 2;
    let head_end = (0..=half).rev().find(|i| raw.is_char_boundary(*i)).unwrap_or(0);
    let tail_start = (raw.len().saturating_sub(half)..raw.len())
        .find(|i| raw.is_char_boundary(*i))
        .unwrap_or(raw.len());
    let elided = raw.len() - head_end - (raw.len() - tail_start);
    format!(
        "{}\n... <{} bytes elided> ...\n{}",
        &raw[..head_end],
        elided,
        &raw[tail_start..]
    )
}

/// Render user variables as HCL-quoted tfvars. Non-secret values get plain strings; the same
/// HCL escaping rules apply to both — qovery_blueprint variable types (string/number/bool) are
/// modeled as strings on the wire, so terraform's type coercion handles the rest at plan time.
fn render_tfvars(request: &BlueprintRequest) -> String {
    request
        .variables
        .iter()
        .map(|v| format!("{} = \"{}\"\n", v.name, hcl_escape(&v.value)))
        .collect()
}

/// Escape an HCL double-quoted string value. Backslash and `"` get the usual `\` escape;
/// `$` and `%` are doubled to suppress HCL string interpolation (`${…}`) and template-directive
/// (`%{…}`) sequences — a variable value like `${DB_PASSWORD}` would otherwise be evaluated by
/// the HCL parser instead of treated as a literal string. Mirrors the catalog template's
/// `hcl_string` filter convention.
fn hcl_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "$$")
        .replace('%', "%%")
}

/// HCL for the kubernetes terraform backend that env-engine terraform services use. Matches
/// `crate::infrastructure::models::cloud_provider::service::get_tfstate_name` — the secret name
/// is `tfstate-default-{service_id}`, and the workspace is "default" by convention.
fn render_kubernetes_backend(tfstate_secret_suffix: &str, namespace: &str) -> String {
    // `secret_suffix` here is the literal suffix value (kubernetes backend prepends the
    // `tfstate-{workspace}-` prefix itself), so we strip the prefix from our computed name.
    let suffix = tfstate_secret_suffix
        .strip_prefix("tfstate-default-")
        .unwrap_or(tfstate_secret_suffix);
    format!(
        r#"terraform {{
  backend "kubernetes" {{
    secret_suffix = "{suffix}"
    namespace     = "{namespace}"
    in_cluster_config = false
  }}
}}
"#
    )
}

/// Shallow recursive copy of `src` into `dst`. Skips dot-directories (`.terraform`, `.git`) to
/// avoid carrying stale state.
fn copy_dir_contents(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&to)?;
            copy_dir_contents(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_diff_payload_passes_through_small_output() {
        let out = TerraformOutput {
            raw_std_output: vec!["hello".to_string(), "world".to_string()],
            raw_error_output: vec![],
        };
        assert_eq!(truncate_diff_payload(&out), "hello\nworld");
    }

    #[test]
    fn truncate_diff_payload_elides_when_oversized() {
        let big_line = "a".repeat(DIFF_PAYLOAD_MAX_BYTES + 1);
        let out = TerraformOutput {
            raw_std_output: vec![big_line],
            raw_error_output: vec![],
        };
        let truncated = truncate_diff_payload(&out);
        assert!(truncated.contains("bytes elided"));
        assert!(truncated.len() <= DIFF_PAYLOAD_MAX_BYTES + 64);
    }

    #[test]
    fn render_tfvars_escapes_quotes_and_backslashes() {
        let req = blueprint_request(vec![("plain", "eu-west-3"), ("nasty", r#"a"b\c"#)]);
        let tfvars = render_tfvars(&req);
        assert!(tfvars.contains(r#"plain = "eu-west-3""#));
        assert!(tfvars.contains(r#"nasty = "a\"b\\c""#));
    }

    #[test]
    fn render_tfvars_escapes_interpolation_and_template_directives() {
        // `${DB_PASSWORD}` would be interpreted as HCL interpolation if not escaped — the parser
        // would try to evaluate the reference (or error). `%{if …}` is template-directive syntax.
        // Both get their leading sigil doubled to keep the value literal.
        let req = blueprint_request(vec![("interp", "${DB_PASSWORD}"), ("template", "%{if x}y%{endif}")]);
        let tfvars = render_tfvars(&req);
        assert!(tfvars.contains(r#"interp = "$${DB_PASSWORD}""#));
        assert!(tfvars.contains(r#"template = "%%{if x}y%%{endif}""#));
    }

    #[test]
    fn render_kubernetes_backend_strips_tfstate_default_prefix() {
        let hcl = render_kubernetes_backend("tfstate-default-abc-123", "env-ns");
        assert!(hcl.contains(r#"secret_suffix = "abc-123""#));
        assert!(hcl.contains(r#"namespace     = "env-ns""#));
    }

    fn blueprint_request(vars: Vec<(&str, &str)>) -> BlueprintRequest {
        use crate::io_models::blueprint::BlueprintVariable;
        BlueprintRequest {
            execution_id: "exec".into(),
            long_id: uuid::Uuid::nil(),
            name: "n".into(),
            kube_name: "n".into(),
            project_long_id: uuid::Uuid::nil(),
            organization_long_id: uuid::Uuid::nil(),
            max_parallel_build: 1,
            max_parallel_deploy: 1,
            variables: vars
                .into_iter()
                .map(|(k, v)| BlueprintVariable {
                    name: k.to_string(),
                    value: v.to_string(),
                    is_secret: false,
                })
                .collect(),
            git_url: String::new(),
            tag: String::new(),
            git_credentials: None,
            git_token_id: None,
            spec_overrides: None,
            qovery_api_token: String::new(),
            environment_id: String::new(),
            import_id: Some("abc-123".into()),
            icon: String::new(),
            env_kube_name: "env-ns".into(),
            backend_type: None,
        }
    }
}
