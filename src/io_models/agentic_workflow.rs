use crate::environment::models;
use crate::environment::models::agentic_workflow::{
    AgenticWorkflowConfig, AgenticWorkflowError, AgenticWorkflowService,
};
use crate::infrastructure::models::cloud_provider::service::Action as DomainAction;
use crate::io_models::context::Context;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::io_models::services_common::GitCredentials;
use base64::Engine;
use base64::engine::general_purpose;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Base64-decode (STANDARD alphabet) a "hardened" wire field into its canonical form.
///
/// q-core base64-encodes the sensitive/opaque agentic-workflow fields — `api_key`, the git
/// access token, `prompt`, `mcp`, output `instructions`, and header values — so they never appear
/// in plaintext in the engine-request JSON (QOV-2086). The engine reverses that here, at the
/// io-model → domain boundary, so everything downstream (env-var construction, prompt ConfigMap)
/// keeps working with canonical values exactly as it did before any encoding existed.
///
/// Strict on purpose: q-core encoding and this decoding ship together (lockstep), so a value that
/// isn't valid base64 / UTF-8 is a contract violation worth failing on, not something to pass
/// through silently. An empty string decodes to an empty string, so unset optional fields are
/// unaffected.
fn decode_hardened_field(field: &str, value: &str) -> Result<String, AgenticWorkflowError> {
    let bytes = general_purpose::STANDARD
        .decode(value)
        .map_err(|e| AgenticWorkflowError::InvalidConfig(format!("field '{field}' is not valid base64: {e}")))?;
    String::from_utf8(bytes).map_err(|e| {
        AgenticWorkflowError::InvalidConfig(format!("field '{field}' does not base64-decode to valid UTF-8: {e}"))
    })
}

/// Default `.spec.activeDeadlineSeconds` for an AgenticWorkflow Job. Agent (Claude Code) tasks
/// routinely run for several minutes, well past the busybox-stub's original 300s default, so
/// this is set generously high. Centralized here (§8.6 TBD - picked a sensible default) rather
/// than duplicated between the io_model default and any chart/domain fallback.
fn default_max_duration_in_sec() -> u64 {
    3_600
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgenticWorkflowModelType {
    Claude,
    Bedrock,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct AgenticWorkflowModel {
    #[serde(rename = "type")]
    pub model_type: AgenticWorkflowModelType,
    /// Write-only credential coming from q-core; never logged or echoed back.
    pub api_key: String,
    /// Opaque JSON blob (e.g. reasoning effort). Interpreted by the agent image, not the engine.
    #[serde(default)]
    pub settings: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct AgenticWorkflowImage {
    pub repository: String,
    pub tag: String,
}

/// Pinned first-cut agent image (`qovery-ai-runner`), manually pushed to the shared registry
/// (§8.4). Used when the wire payload omits `image`, so the engine has a concrete default even
/// if q-core doesn't dictate one. Bump the tag here (and q-core's `AGENTIC_WORKFLOW_IMAGE_TAG`)
/// when a new image is published, until the per-workflow `Build` path (§8.3) makes this dynamic.
fn default_image() -> AgenticWorkflowImage {
    AgenticWorkflowImage {
        repository: "public.ecr.aws/r3m4q3r9/qovery-ai-runner".to_string(),
        tag: "0.0.2".to_string(),
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct AgenticWorkflowProjectRepository {
    pub url: String,
    pub branch: String,
    /// Mirrors q-job's `JobSource::Docker::git_credentials` token handling: q-core resolves
    /// `gitTokenId` into a short-lived credential before it ever reaches the engine.
    #[serde(default)]
    pub git_credentials: Option<GitCredentials>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct AgenticWorkflowHeader {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct AgenticWorkflowOutput {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Vec<AgenticWorkflowHeader>,
    /// Free-text guidance fed to the agent at runtime to shape what it produces for this sink
    /// (resolved §8.7) - not an HTTP delivery detail.
    #[serde(default)]
    pub instructions: String,
}

/// `AgenticWorkflow` wire payload sent by q-core's `EngineRequest.AgenticWorkflow` (see
/// `q-core/.../deployment/model/EngineRequest.kt`). Field names are plain snake_case on
/// purpose: q-core serializes with Jackson's `PropertyNamingStrategies.SNAKE_CASE`, which
/// turns e.g. Kotlin's `outputVariableValidationPattern` into `output_variable_validation_pattern`
/// - matching these Rust field names with no `#[serde(rename_all)]` needed.
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct AgenticWorkflow {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,

    #[serde(default = "default_image")]
    pub image: AgenticWorkflowImage,
    /// Extra Dockerfile fragment injected at build time (§8.3). Not yet consumed: the engine
    /// currently always runs the manually-pushed base image referenced by `image`. Carried
    /// through end-to-end so the domain model / chart are already shaped for the future
    /// per-workflow `Build` path.
    #[serde(default)]
    pub docker_fragment: String,

    pub prompt: String,
    pub model: AgenticWorkflowModel,
    /// Raw JSON blob describing MCP servers, opaque to the engine (mirrors q-core's `mcp: String`).
    #[serde(default)]
    pub mcp: String,
    #[serde(default)]
    pub project_repositories: Vec<AgenticWorkflowProjectRepository>,
    /// Hostnames the agent is allowed to reach. Egress enforcement (mitmproxy) is deferred
    /// (§8.2); this is threaded through end-to-end so it's ready to be wired up.
    #[serde(default)]
    pub host_allowlist: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<AgenticWorkflowOutput>,

    pub cpu_request_in_milli: u32,
    #[serde(default)]
    pub cpu_limit_in_milli: Option<u32>,
    pub ram_request_in_mib: u32,
    pub ram_limit_in_mib: u32,

    pub output_variable_validation_pattern: String,
    #[serde(default = "default_max_duration_in_sec")]
    pub max_duration_in_sec: u64,
}

impl AgenticWorkflow {
    pub fn to_agentic_workflow_domain(
        self,
        context: &Context,
    ) -> Result<Box<dyn AgenticWorkflowService>, AgenticWorkflowError> {
        let long_id = self.long_id;
        let name = self.name.clone();
        let kube_name = self.kube_name.clone();
        let config = self.into_domain_config()?;

        let service = models::agentic_workflow::AgenticWorkflow::new(
            context,
            long_id,
            name,
            kube_name,
            config,
            // Hardcoded on purpose: only full-environment delete (which calls `on_delete`
            // directly) is supported today, not per-service removal during an environment
            // update. If an AgenticWorkflow is ever removed individually rather than the whole
            // environment, this hardcoded `Create` means the Job would be left orphaned instead
            // of cleaned up.
            DomainAction::Create,
            |transmitter| context.get_event_details(transmitter),
        )?;

        Ok(Box::new(service))
    }

    /// Build the domain [`AgenticWorkflowConfig`], reversing q-core's transit base64-encoding on
    /// the hardened fields (see [`decode_hardened_field`]). Split out from
    /// `to_agentic_workflow_domain` so the full decode wiring is unit-testable without
    /// constructing a `Context`.
    fn into_domain_config(self) -> Result<AgenticWorkflowConfig, AgenticWorkflowError> {
        // Repos/outputs are built before the struct so the fallible per-repo / per-output decodes
        // can fold their errors in one pass (`collect::<Result<Vec<_>>>()`).
        let project_repositories = self
            .project_repositories
            .into_iter()
            .map(|repo| {
                let git_token = repo
                    .git_credentials
                    .map(|creds| decode_hardened_field("git_credentials.access_token", &creds.access_token))
                    .transpose()?;
                Ok(models::agentic_workflow::AgenticWorkflowProjectRepository {
                    url: repo.url,
                    branch: repo.branch,
                    git_token,
                })
            })
            .collect::<Result<Vec<_>, AgenticWorkflowError>>()?;

        let outputs = self
            .outputs
            .into_iter()
            .map(|output| {
                let headers = output
                    .headers
                    .into_iter()
                    .map(|h| Ok((h.name, decode_hardened_field("outputs.headers.value", &h.value)?)))
                    .collect::<Result<Vec<_>, AgenticWorkflowError>>()?;
                Ok(models::agentic_workflow::AgenticWorkflowOutput {
                    name: output.name,
                    url: output.url,
                    headers,
                    instructions: decode_hardened_field("outputs.instructions", &output.instructions)?,
                })
            })
            .collect::<Result<Vec<_>, AgenticWorkflowError>>()?;

        Ok(AgenticWorkflowConfig {
            image_repository: self.image.repository,
            image_tag: self.image.tag,
            docker_fragment: self.docker_fragment,
            prompt: decode_hardened_field("prompt", &self.prompt)?,
            model_type: self.model.model_type,
            model_api_key: decode_hardened_field("model.api_key", &self.model.api_key)?,
            model_settings: self.model.settings,
            mcp: decode_hardened_field("mcp", &self.mcp)?,
            project_repositories,
            host_allowlist: self.host_allowlist,
            outputs,
            cpu_request_in_milli: KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
            cpu_limit_in_milli: self.cpu_limit_in_milli.map(KubernetesCpuResourceUnit::MilliCpu),
            ram_request_in_mib: KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
            ram_limit_in_mib: KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
            output_variable_validation_pattern: self.output_variable_validation_pattern,
            max_duration_in_sec: self.max_duration_in_sec,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden-JSON contract-sync test (plan §D4, the highest-risk seam): this is the exact JSON
    /// produced by q-core's `EngineRequestUnitTest."should serialize agentic workflow using
    /// exactly the field set the engine io-model expects"` test
    /// (corenetto/src/test/kotlin/.../EngineRequestUnitTest.kt), via `RedisEngineService.objectMapper`
    /// (Jackson `PropertyNamingStrategies.SNAKE_CASE`). If this ever fails to deserialize, or the
    /// q-core golden literal drifts from this one, the wire contract between the two repos is broken.
    // The "hardened" fields (`prompt`, `api_key`, `mcp`, git `access_token`, output
    // `instructions`, header `value`) are base64-encoded on the wire by q-core (QOV-2086) so
    // secrets/opaque blobs never appear in plaintext in the request JSON. The io-model holds them
    // as-received (still encoded); the engine reverses the encoding in `to_agentic_workflow_domain`.
    // Decoded values here: prompt="Investigate the incident and summarize root cause.",
    // api_key="sk-secret", mcp={"servers":[]}, access_token="resolved-token",
    // header value="application/json", instructions="Keep it under 500 characters.".
    const GOLDEN_JSON: &str = r#"{"long_id":"eb5163b9-0e4c-4c9a-b304-9b984c85337d","name":"my-agentic-workflow","kube_name":"agentic-workflow-zeb5163b9","image":{"repository":"public.ecr.aws/r3m4q3r9/qovery-ai-runner","tag":"0.0.1"},"docker_fragment":"RUN apt-get install -y jq","prompt":"SW52ZXN0aWdhdGUgdGhlIGluY2lkZW50IGFuZCBzdW1tYXJpemUgcm9vdCBjYXVzZS4=","model":{"type":"CLAUDE","api_key":"c2stc2VjcmV0","settings":"{\"effort\":\"high\"}"},"mcp":"eyJzZXJ2ZXJzIjpbXX0=","project_repositories":[{"url":"https://github.com/qovery/demo","branch":"main","git_credentials":{"login":"x-access-token","access_token":"cmVzb2x2ZWQtdG9rZW4=","expired_at":"1970-01-01T00:00:00Z"}}],"host_allowlist":["api.github.com"],"outputs":[{"name":"slack","url":"https://hooks.slack.com/services/x","headers":[{"name":"Content-Type","value":"YXBwbGljYXRpb24vanNvbg=="}],"instructions":"S2VlcCBpdCB1bmRlciA1MDAgY2hhcmFjdGVycy4="}],"cpu_request_in_milli":500,"cpu_limit_in_milli":1000,"ram_request_in_mib":512,"ram_limit_in_mib":1024,"output_variable_validation_pattern":"^[a-zA-Z_][a-zA-Z0-9_]*$","max_duration_in_sec":3600}"#;

    /// Same as GOLDEN_JSON's companion "defaulted fields" q-core test: the optional fields
    /// (docker_fragment/mcp/project_repositories/host_allowlist/outputs/cpu_limit_in_milli) are
    /// present but empty/null - proving `#[serde(default)]` isn't masking a real mismatch.
    const GOLDEN_JSON_DEFAULTS: &str = r#"{"long_id":"eb5163b9-0e4c-4c9a-b304-9b984c85337d","name":"my-agentic-workflow","kube_name":"agentic-workflow-zeb5163b9","image":{"repository":"public.ecr.aws/r3m4q3r9/qovery-ai-runner","tag":"0.0.1"},"docker_fragment":"","prompt":"","model":{"type":"CLAUDE","api_key":"","settings":""},"mcp":"","project_repositories":[],"host_allowlist":[],"outputs":[],"cpu_request_in_milli":500,"cpu_limit_in_milli":null,"ram_request_in_mib":512,"ram_limit_in_mib":1024,"output_variable_validation_pattern":"^[a-zA-Z_][a-zA-Z0-9_]*$","max_duration_in_sec":3600}"#;

    #[test]
    fn deserializes_the_q_core_golden_json_contract() {
        let workflow: AgenticWorkflow =
            serde_json::from_str(GOLDEN_JSON).expect("golden JSON from q-core should deserialize");

        assert_eq!(workflow.long_id.to_string(), "eb5163b9-0e4c-4c9a-b304-9b984c85337d");
        assert_eq!(workflow.name, "my-agentic-workflow");
        assert_eq!(workflow.kube_name, "agentic-workflow-zeb5163b9");
        assert_eq!(workflow.image.repository, "public.ecr.aws/r3m4q3r9/qovery-ai-runner");
        assert_eq!(workflow.image.tag, "0.0.1");
        assert_eq!(workflow.docker_fragment, "RUN apt-get install -y jq");
        // Hardened fields are held as-received (base64); the engine decodes them at the
        // io-model → domain boundary via `decode_hardened_field` (see its unit tests below).
        assert_eq!(
            workflow.prompt,
            "SW52ZXN0aWdhdGUgdGhlIGluY2lkZW50IGFuZCBzdW1tYXJpemUgcm9vdCBjYXVzZS4="
        );
        assert_eq!(workflow.model.model_type, AgenticWorkflowModelType::Claude);
        assert_eq!(workflow.model.api_key, "c2stc2VjcmV0");
        assert_eq!(workflow.model.settings, "{\"effort\":\"high\"}");
        assert_eq!(workflow.mcp, "eyJzZXJ2ZXJzIjpbXX0=");
        assert_eq!(workflow.project_repositories.len(), 1);
        assert_eq!(workflow.project_repositories[0].url, "https://github.com/qovery/demo");
        assert_eq!(workflow.project_repositories[0].branch, "main");
        let git_credentials = workflow.project_repositories[0]
            .git_credentials
            .as_ref()
            .expect("git_credentials should be present");
        assert_eq!(git_credentials.login, "x-access-token");
        assert_eq!(git_credentials.access_token, "cmVzb2x2ZWQtdG9rZW4=");
        assert_eq!(workflow.host_allowlist, vec!["api.github.com".to_string()]);
        assert_eq!(workflow.outputs.len(), 1);
        assert_eq!(workflow.outputs[0].name, "slack");
        assert_eq!(workflow.outputs[0].url.as_deref(), Some("https://hooks.slack.com/services/x"));
        assert_eq!(workflow.outputs[0].headers[0].name, "Content-Type");
        assert_eq!(workflow.outputs[0].headers[0].value, "YXBwbGljYXRpb24vanNvbg==");
        assert_eq!(workflow.outputs[0].instructions, "S2VlcCBpdCB1bmRlciA1MDAgY2hhcmFjdGVycy4=");
        assert_eq!(workflow.cpu_request_in_milli, 500);
        assert_eq!(workflow.cpu_limit_in_milli, Some(1000));
        assert_eq!(workflow.ram_request_in_mib, 512);
        assert_eq!(workflow.ram_limit_in_mib, 1024);
        assert_eq!(workflow.output_variable_validation_pattern, "^[a-zA-Z_][a-zA-Z0-9_]*$");
        assert_eq!(workflow.max_duration_in_sec, 3600);
    }

    #[test]
    fn deserializes_the_q_core_golden_json_contract_with_defaulted_optional_fields() {
        let workflow: AgenticWorkflow = serde_json::from_str(GOLDEN_JSON_DEFAULTS)
            .expect("golden JSON with defaults from q-core should deserialize");

        assert_eq!(workflow.docker_fragment, "");
        assert_eq!(workflow.mcp, "");
        assert!(workflow.project_repositories.is_empty());
        assert!(workflow.host_allowlist.is_empty());
        assert!(workflow.outputs.is_empty());
        assert_eq!(workflow.cpu_limit_in_milli, None);
    }

    #[test]
    fn image_defaults_to_the_pinned_agent_image_when_omitted() {
        // The wire payload may omit `image`; the engine then falls back to the pinned
        // first-cut agent image (`default_image`). Build a payload without the `image` key.
        let without_image = GOLDEN_JSON_DEFAULTS.replacen(
            r#""image":{"repository":"public.ecr.aws/r3m4q3r9/qovery-ai-runner","tag":"0.0.1"},"#,
            "",
            1,
        );
        let workflow: AgenticWorkflow =
            serde_json::from_str(&without_image).expect("payload without image should deserialize");
        assert_eq!(workflow.image, default_image());
        assert_eq!(workflow.image.tag, "0.0.2");
    }

    #[test]
    fn round_trips_the_golden_json_contract() {
        let workflow: AgenticWorkflow =
            serde_json::from_str(GOLDEN_JSON).expect("golden JSON from q-core should deserialize");
        let reserialized = serde_json::to_value(&workflow).expect("should reserialize");
        let original: serde_json::Value = serde_json::from_str(GOLDEN_JSON).unwrap();

        assert_eq!(reserialized, original);
    }

    #[test]
    fn into_domain_config_decodes_every_hardened_field() {
        // Guards the decode wiring: if any hardened field's `decode_hardened_field` call were
        // dropped in `into_domain_config`, its value would stay base64 here (and the container
        // would then receive a double-encoded value). The golden's hardened fields are base64.
        let workflow: AgenticWorkflow = serde_json::from_str(GOLDEN_JSON).expect("golden JSON should deserialize");
        let config = workflow.into_domain_config().expect("domain config should build");

        assert_eq!(config.prompt, "Investigate the incident and summarize root cause.");
        assert_eq!(config.model_api_key, "sk-secret");
        assert_eq!(config.mcp, "{\"servers\":[]}");
        // NOT hardened — must pass through unchanged (a spurious decode here would corrupt it).
        assert_eq!(config.model_settings, "{\"effort\":\"high\"}");
        assert_eq!(config.project_repositories[0].git_token.as_deref(), Some("resolved-token"));
        assert_eq!(config.outputs[0].instructions, "Keep it under 500 characters.");
        assert_eq!(
            config.outputs[0].headers,
            vec![("Content-Type".to_string(), "application/json".to_string())]
        );
    }

    #[test]
    fn decode_hardened_field_decodes_a_base64_value() {
        assert_eq!(decode_hardened_field("api_key", "c2stc2VjcmV0").unwrap(), "sk-secret");
    }

    #[test]
    fn decode_hardened_field_maps_empty_to_empty() {
        // Unset optional hardened fields arrive as an empty string and must stay empty, not error.
        assert_eq!(decode_hardened_field("mcp", "").unwrap(), "");
    }

    #[test]
    fn decode_hardened_field_errors_on_invalid_base64() {
        // A raw plaintext key ('-' isn't in the STANDARD alphabet) is a contract violation under
        // the lockstep encoding: q-core must send it base64-encoded.
        let err = decode_hardened_field("api_key", "sk-ant-plaintext").unwrap_err();
        assert!(matches!(err, AgenticWorkflowError::InvalidConfig(_)));
    }

    #[test]
    fn decode_hardened_field_errors_on_valid_base64_that_is_not_utf8() {
        // "//4=" is valid base64 for the bytes 0xFF 0xFE, which are not valid UTF-8.
        let err = decode_hardened_field("prompt", "//4=").unwrap_err();
        assert!(matches!(err, AgenticWorkflowError::InvalidConfig(_)));
    }
}
