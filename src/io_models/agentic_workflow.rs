use crate::environment::models;
use crate::environment::models::agentic_workflow::{
    AgenticWorkflowConfig, AgenticWorkflowError, AgenticWorkflowService,
};
use crate::infrastructure::models::cloud_provider::service::Action as DomainAction;
use crate::io_models::Action;
use crate::io_models::context::Context;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::io_models::services_common::GitCredentials;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Default `.spec.activeDeadlineSeconds` for an AgenticWorkflow Job. Agent (Claude Code) tasks
/// routinely run for several minutes, well past the busybox-stub's original 300s default, so
/// this is set generously high. Centralized here (§8.6 TBD - picked a sensible default) rather
/// than duplicated between the io_model default and any chart/domain fallback.
fn default_max_duration_in_sec() -> u64 {
    3_600
}

/// Spelled out because `Action` has no `Default` impl. Defaulting at all is what keeps the two repos
/// deployable in either order: against a q-core not sending `action` yet, the engine behaves as before.
fn default_action() -> Action {
    Action::Create
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

/// Mirror of webhook-receiver's `MAX_GENERIC_HOOK_BODY_BYTES`, held only to enforce the ordering
/// asserted below. It lives in another repo and can therefore drift; the assertion is what turns
/// drift into a build failure instead of a runtime surprise.
const WEBHOOK_RECEIVER_MAX_BODY_BYTES: usize = 256 * 1024;

/// Bounds body + headers, so it must stay **looser** than webhook-receiver's body-only cap: were
/// they equal, a body accepted at the edge would fail here as soon as it carried one header. The
/// headroom keeps this guard unreachable via the webhook path, leaving it as defence in depth for
/// other callers. Upper bound is the ConfigMap it shares with `prompt.txt` (~1 MiB), measured
/// base64-inflated: 384 KiB becomes ~512 KiB.
pub const MAX_RUN_PAYLOAD_BYTES: usize = 384 * 1024;

const _: () = assert!(WEBHOOK_RECEIVER_MAX_BODY_BYTES < MAX_RUN_PAYLOAD_BYTES);

/// The webhook event a run is reacting to (QOV-2084), as opposed to the workflow's static config.
///
/// Like the workflow's static fields, `body` and header values arrive verbatim and are used as-is.
/// Header names are also plain, so [`AgenticWorkflowHeader`] is plain end to end.
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct AgenticWorkflowRunPayload {
    pub body: String,
    /// A `Vec` because HTTP permits repeated header names.
    #[serde(default)]
    pub headers: Vec<AgenticWorkflowHeader>,
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
    #[serde(default = "default_action")]
    pub action: Action,

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

    /// Absent for a deploy with no triggering event. `#[serde(default)]` keeps the engine
    /// deployable against a q-core that does not send it yet.
    #[serde(default)]
    pub payload: Option<AgenticWorkflowRunPayload>,
}

impl AgenticWorkflow {
    pub fn to_agentic_workflow_domain(
        self,
        context: &Context,
    ) -> Result<Box<dyn AgenticWorkflowService>, AgenticWorkflowError> {
        let long_id = self.long_id;
        let name = self.name.clone();
        let kube_name = self.kube_name.clone();
        let action = self.action;
        let config = self.into_domain_config()?;

        let service = models::agentic_workflow::AgenticWorkflow::new(
            context,
            long_id,
            name,
            kube_name,
            config,
            DomainAction::from(action),
            |transmitter| context.get_event_details(transmitter),
        )?;

        Ok(Box::new(service))
    }

    /// Build the domain [`AgenticWorkflowConfig`] from q-core's plain JSON wire fields. Split out
    /// from `to_agentic_workflow_domain` so the conversion is unit-testable without constructing a
    /// `Context`.
    fn into_domain_config(self) -> Result<AgenticWorkflowConfig, AgenticWorkflowError> {
        let project_repositories = self
            .project_repositories
            .into_iter()
            .map(|repo| models::agentic_workflow::AgenticWorkflowProjectRepository {
                url: repo.url,
                branch: repo.branch,
                git_token: repo.git_credentials.map(|creds| creds.access_token),
            })
            .collect::<Vec<_>>();

        let outputs = self
            .outputs
            .into_iter()
            .map(|output| models::agentic_workflow::AgenticWorkflowOutput {
                name: output.name,
                url: output.url,
                headers: output.headers.into_iter().map(|h| (h.name, h.value)).collect(),
                instructions: output.instructions,
            })
            .collect::<Vec<_>>();

        let run_payload = self
            .payload
            // q-core always sends the field, using an empty payload for "no triggering event", so
            // emptiness — not absence — is what must collapse to `None` here. Otherwise every
            // manually deployed workflow would render empty `WEBHOOK_*` inputs, and ai-runner would
            // append a stray `# INPUTS` block to its prompt.
            .filter(|payload| !(payload.body.is_empty() && payload.headers.is_empty()))
            .map(|payload| {
                // Verbatim: q-core sends the payload as plain JSON.
                let body = payload.body;
                let headers = payload
                    .headers
                    .into_iter()
                    .map(|h| (h.name, h.value))
                    .collect::<Vec<(String, String)>>();

                let total_bytes = body.len()
                    + headers
                        .iter()
                        .map(|(name, value)| name.len() + value.len())
                        .sum::<usize>();
                if total_bytes > MAX_RUN_PAYLOAD_BYTES {
                    return Err(AgenticWorkflowError::InvalidConfig(format!(
                        "run payload is {total_bytes} bytes, over the {MAX_RUN_PAYLOAD_BYTES} byte limit"
                    )));
                }

                Ok(models::agentic_workflow::AgenticWorkflowRunPayload { body, headers })
            })
            .transpose()?;

        Ok(AgenticWorkflowConfig {
            image_repository: self.image.repository,
            image_tag: self.image.tag,
            docker_fragment: self.docker_fragment,
            prompt: self.prompt,
            model_type: self.model.model_type,
            model_api_key: self.model.api_key,
            model_settings: self.model.settings,
            mcp: self.mcp,
            project_repositories,
            host_allowlist: self.host_allowlist,
            outputs,
            cpu_request_in_milli: KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
            cpu_limit_in_milli: self.cpu_limit_in_milli.map(KubernetesCpuResourceUnit::MilliCpu),
            ram_request_in_mib: KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
            ram_limit_in_mib: KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
            output_variable_validation_pattern: self.output_variable_validation_pattern,
            max_duration_in_sec: self.max_duration_in_sec,
            run_payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden-JSON contract-sync test, the highest-risk seam of the integration: this is the exact JSON
    /// produced by q-core's `EngineRequestUnitTest."should serialize agentic workflow using
    /// exactly the field set the engine io-model expects"` test
    /// (corenetto/src/test/kotlin/.../EngineRequestUnitTest.kt), via `RedisEngineService.objectMapper`
    /// (Jackson `PropertyNamingStrategies.SNAKE_CASE`). If this ever fails to deserialize, or the
    /// q-core golden literal drifts from this one, the wire contract between the two repos is broken.
    // Static agentic-workflow fields are plain JSON strings on the q-core/engine wire contract.
    // The run payload is likewise plain JSON and reaches the domain configuration verbatim.
    const GOLDEN_JSON: &str = r#"{"long_id":"eb5163b9-0e4c-4c9a-b304-9b984c85337d","name":"my-agentic-workflow","kube_name":"agentic-workflow-zeb5163b9-my-agentic-workflow","action":"CREATE","image":{"repository":"public.ecr.aws/r3m4q3r9/qovery-ai-runner","tag":"0.0.1"},"docker_fragment":"RUN apt-get install -y jq","prompt":"Investigate the incident and summarize root cause.","model":{"type":"CLAUDE","api_key":"sk-secret","settings":"{\"effort\":\"high\"}"},"mcp":"{\"servers\":[]}","project_repositories":[{"url":"https://github.com/qovery/demo","branch":"main","git_credentials":{"login":"x-access-token","access_token":"resolved-token","expired_at":"1970-01-01T00:00:00Z"}}],"host_allowlist":["api.github.com"],"outputs":[{"name":"slack","url":"https://hooks.slack.com/services/x","headers":[{"name":"Content-Type","value":"application/json"}],"instructions":"Keep it under 500 characters."}],"cpu_request_in_milli":500,"cpu_limit_in_milli":1000,"ram_request_in_mib":512,"ram_limit_in_mib":1024,"output_variable_validation_pattern":"^[a-zA-Z_][a-zA-Z0-9_]*$","max_duration_in_sec":3600,"payload":{"body":"{\"issue\":{\"key\":\"QOV-1\"}}","headers":[{"name":"x-atlassian-webhook-identifier","value":"abc-123"},{"name":"content-type","value":"application/json"}]}}"#;

    /// Same as GOLDEN_JSON's companion "defaulted fields" q-core test: the optional fields
    /// (docker_fragment/mcp/project_repositories/host_allowlist/outputs/cpu_limit_in_milli) are
    /// present but empty/null - proving `#[serde(default)]` isn't masking a real mismatch.
    const GOLDEN_JSON_DEFAULTS: &str = r#"{"long_id":"eb5163b9-0e4c-4c9a-b304-9b984c85337d","name":"my-agentic-workflow","kube_name":"agentic-workflow-zeb5163b9-my-agentic-workflow","action":"CREATE","image":{"repository":"public.ecr.aws/r3m4q3r9/qovery-ai-runner","tag":"0.0.1"},"docker_fragment":"","prompt":"","model":{"type":"CLAUDE","api_key":"","settings":""},"mcp":"","project_repositories":[],"host_allowlist":[],"outputs":[],"cpu_request_in_milli":500,"cpu_limit_in_milli":null,"ram_request_in_mib":512,"ram_limit_in_mib":1024,"output_variable_validation_pattern":"^[a-zA-Z_][a-zA-Z0-9_]*$","max_duration_in_sec":3600,"payload":{"body":"","headers":[]}}"#;

    #[test]
    fn deserializes_the_q_core_golden_json_contract() {
        let workflow: AgenticWorkflow =
            serde_json::from_str(GOLDEN_JSON).expect("golden JSON from q-core should deserialize");

        assert_eq!(workflow.long_id.to_string(), "eb5163b9-0e4c-4c9a-b304-9b984c85337d");
        assert_eq!(workflow.name, "my-agentic-workflow");
        assert_eq!(workflow.kube_name, "agentic-workflow-zeb5163b9-my-agentic-workflow");
        // Compared through the domain action because `io_models::Action` derives no `Debug`.
        assert_eq!(DomainAction::from(workflow.action), DomainAction::Create);
        assert_eq!(workflow.image.repository, "public.ecr.aws/r3m4q3r9/qovery-ai-runner");
        assert_eq!(workflow.image.tag, "0.0.1");
        assert_eq!(workflow.docker_fragment, "RUN apt-get install -y jq");
        assert_eq!(workflow.prompt, "Investigate the incident and summarize root cause.");
        assert_eq!(workflow.model.model_type, AgenticWorkflowModelType::Claude);
        assert_eq!(workflow.model.api_key, "sk-secret");
        assert_eq!(workflow.model.settings, "{\"effort\":\"high\"}");
        assert_eq!(workflow.mcp, "{\"servers\":[]}");
        assert_eq!(workflow.project_repositories.len(), 1);
        assert_eq!(workflow.project_repositories[0].url, "https://github.com/qovery/demo");
        assert_eq!(workflow.project_repositories[0].branch, "main");
        let git_credentials = workflow.project_repositories[0]
            .git_credentials
            .as_ref()
            .expect("git_credentials should be present");
        assert_eq!(git_credentials.login, "x-access-token");
        assert_eq!(git_credentials.access_token, "resolved-token");
        assert_eq!(workflow.host_allowlist, vec!["api.github.com".to_string()]);
        assert_eq!(workflow.outputs.len(), 1);
        assert_eq!(workflow.outputs[0].name, "slack");
        assert_eq!(workflow.outputs[0].url.as_deref(), Some("https://hooks.slack.com/services/x"));
        assert_eq!(workflow.outputs[0].headers[0].name, "Content-Type");
        assert_eq!(workflow.outputs[0].headers[0].value, "application/json");
        assert_eq!(workflow.outputs[0].instructions, "Keep it under 500 characters.");
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
    fn into_domain_config_preserves_plain_static_fields_verbatim() {
        let workflow: AgenticWorkflow = serde_json::from_str(GOLDEN_JSON).expect("golden JSON should deserialize");
        let config = workflow.into_domain_config().expect("domain config should build");

        assert_eq!(config.prompt, "Investigate the incident and summarize root cause.");
        assert_eq!(config.model_api_key, "sk-secret");
        assert_eq!(config.mcp, "{\"servers\":[]}");
        assert_eq!(config.model_settings, "{\"effort\":\"high\"}");
        assert_eq!(config.project_repositories[0].git_token.as_deref(), Some("resolved-token"));
        assert_eq!(config.outputs[0].instructions, "Keep it under 500 characters.");
        assert_eq!(
            config.outputs[0].headers,
            vec![("Content-Type".to_string(), "application/json".to_string())]
        );
    }

    #[test]
    fn wire_action_drives_the_domain_action() {
        // Guards QOV-2086: this was hardcoded to `Create`, so `on_delete` was unreachable and a
        // workflow removed on its own left its Job orphaned.
        let deleted_json = GOLDEN_JSON.replacen(r#""action":"CREATE""#, r#""action":"DELETE""#, 1);
        let workflow: AgenticWorkflow = serde_json::from_str(&deleted_json).expect("should deserialize");

        assert_eq!(DomainAction::from(workflow.action), DomainAction::Delete);
    }

    #[test]
    fn action_defaults_to_create_when_the_wire_payload_omits_it() {
        let without_action = GOLDEN_JSON.replacen(r#""action":"CREATE","#, "", 1);
        let workflow: AgenticWorkflow =
            serde_json::from_str(&without_action).expect("payload without action should deserialize");

        assert_eq!(DomainAction::from(workflow.action), DomainAction::Create);
    }

    #[test]
    fn deserializes_the_run_payload_body_and_headers() {
        let workflow: AgenticWorkflow = serde_json::from_str(GOLDEN_JSON).expect("golden JSON should deserialize");
        let payload = workflow.payload.as_ref().expect("payload should be present");

        assert_eq!(payload.body, "{\"issue\":{\"key\":\"QOV-1\"}}");
        assert_eq!(payload.headers.len(), 2);
        assert_eq!(payload.headers[0].name, "x-atlassian-webhook-identifier");
        assert_eq!(payload.headers[0].value, "abc-123");
    }

    #[test]
    fn payload_is_none_when_the_wire_payload_omits_it() {
        // Deployability in either order.
        let without_payload = GOLDEN_JSON_DEFAULTS.replacen(r#","payload":{"body":"","headers":[]}"#, "", 1);
        let workflow: AgenticWorkflow =
            serde_json::from_str(&without_payload).expect("payload-less request should deserialize");

        assert!(workflow.payload.is_none());
    }

    /// q-core sends the payload unconditionally and expresses "no triggering event" as an empty one,
    /// so an empty payload must collapse to no run payload at all. Were it carried through, every
    /// manual deploy would render empty `WEBHOOK_*` inputs and ai-runner would append a stray
    /// `# INPUTS` block to the prompt.
    #[test]
    fn an_empty_wire_payload_produces_no_run_payload() {
        let workflow: AgenticWorkflow =
            serde_json::from_str(GOLDEN_JSON_DEFAULTS).expect("defaults golden should deserialize");
        assert!(workflow.payload.is_some(), "q-core always sends the field");

        let config = workflow.into_domain_config().expect("domain config should build");

        assert!(config.run_payload.is_none());
    }

    /// The payload must reach the domain untouched, including values that happen to parse as base64.
    #[test]
    fn into_domain_config_carries_the_run_payload_verbatim() {
        let workflow: AgenticWorkflow = serde_json::from_str(GOLDEN_JSON).expect("golden JSON should deserialize");
        let config = workflow.into_domain_config().expect("domain config should build");
        let payload = config.run_payload.expect("run payload should be present");

        assert_eq!(payload.body, "{\"issue\":{\"key\":\"QOV-1\"}}");
        assert_eq!(
            payload.headers,
            vec![
                ("x-atlassian-webhook-identifier".to_string(), "abc-123".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ]
        );
    }

    /// The exact value a decode-on-receive engine would silently corrupt: `"abcd"` is both plausible
    /// webhook content and valid base64 (decoding to non-UTF-8 bytes).
    #[test]
    fn a_body_that_looks_like_base64_is_not_decoded() {
        let json = GOLDEN_JSON.replacen(r#"{\"issue\":{\"key\":\"QOV-1\"}}"#, "abcd", 1);
        let workflow: AgenticWorkflow = serde_json::from_str(&json).expect("should deserialize");

        let config = workflow.into_domain_config().expect("domain config should build");

        assert_eq!(config.run_payload.expect("run payload should be present").body, "abcd");
    }

    /// The ordering of the two caps is asserted at compile time; this covers the behaviour that
    /// ordering exists for - a body accepted at the edge, plus headers, must still deploy.
    #[test]
    fn a_body_at_the_receiver_limit_plus_headers_still_builds_a_domain_config() {
        let body = "x".repeat(WEBHOOK_RECEIVER_MAX_BODY_BYTES);
        let headers =
            r#"[{"name":"authorization","value":"Bearer token"},{"name":"content-type","value":"application/json"}]"#;
        let json = GOLDEN_JSON
            .replacen(r#"{\"issue\":{\"key\":\"QOV-1\"}}"#, &body, 1)
            .replacen(
                r#"[{"name":"x-atlassian-webhook-identifier","value":"abc-123"},{"name":"content-type","value":"application/json"}]"#,
                headers,
                1,
            );
        let workflow: AgenticWorkflow = serde_json::from_str(&json).expect("should deserialize");

        let config = workflow
            .into_domain_config()
            .expect("a body at webhook-receiver's limit plus headers must not be rejected here");

        let payload = config.run_payload.expect("run payload should be present");
        assert_eq!(payload.body.len(), WEBHOOK_RECEIVER_MAX_BODY_BYTES);
    }

    #[test]
    fn into_domain_config_rejects_a_run_payload_over_the_size_cap() {
        // Failing here with a clear message beats a Kubernetes rejection at apply time.
        let oversized = "x".repeat(MAX_RUN_PAYLOAD_BYTES + 1);
        let json = GOLDEN_JSON.replacen(r#"{\"issue\":{\"key\":\"QOV-1\"}}"#, &oversized, 1);
        let workflow: AgenticWorkflow = serde_json::from_str(&json).expect("should deserialize");

        let err = workflow.into_domain_config().unwrap_err();

        assert!(matches!(err, AgenticWorkflowError::InvalidConfig(_)));
        assert!(format!("{err}").contains("run payload"));
    }

    #[test]
    fn into_domain_config_preserves_base64_looking_static_fields_verbatim() {
        let json = GOLDEN_JSON
            .replacen(
                "Investigate the incident and summarize root cause.",
                "SW52ZXN0aWdhdGUgdGhlIGluY2lkZW50IGFuZCBzdW1tYXJpemUgcm9vdCBjYXVzZS4=",
                1,
            )
            .replacen("sk-secret", "c2stc2VjcmV0", 1)
            .replacen(r#"{\"servers\":[]}"#, "eyJzZXJ2ZXJzIjpbXX0=", 1)
            .replacen("resolved-token", "cmVzb2x2ZWQtdG9rZW4=", 1)
            .replacen("application/json", "YXBwbGljYXRpb24vanNvbg==", 1)
            .replacen("Keep it under 500 characters.", "S2VlcCBpdCB1bmRlciA1MDAgY2hhcmFjdGVycy4=", 1);
        let workflow: AgenticWorkflow = serde_json::from_str(&json).expect("base64-looking JSON should deserialize");

        let config = workflow.into_domain_config().expect("domain config should build");

        assert_eq!(
            config.prompt,
            "SW52ZXN0aWdhdGUgdGhlIGluY2lkZW50IGFuZCBzdW1tYXJpemUgcm9vdCBjYXVzZS4="
        );
        assert_eq!(config.model_api_key, "c2stc2VjcmV0");
        assert_eq!(config.mcp, "eyJzZXJ2ZXJzIjpbXX0=");
        assert_eq!(
            config.project_repositories[0].git_token.as_deref(),
            Some("cmVzb2x2ZWQtdG9rZW4=")
        );
        assert_eq!(config.outputs[0].instructions, "S2VlcCBpdCB1bmRlciA1MDAgY2hhcmFjdGVycy4=");
        assert_eq!(
            config.outputs[0].headers,
            vec![("Content-Type".to_string(), "YXBwbGljYXRpb24vanNvbg==".to_string())]
        );
    }

    #[test]
    fn into_domain_config_preserves_empty_static_fields_verbatim() {
        let workflow: AgenticWorkflow =
            serde_json::from_str(GOLDEN_JSON_DEFAULTS).expect("defaults golden should deserialize");

        let config = workflow.into_domain_config().expect("domain config should build");

        assert_eq!(config.prompt, "");
        assert_eq!(config.model_api_key, "");
        assert_eq!(config.mcp, "");
    }

    #[test]
    fn into_domain_config_preserves_empty_repository_and_output_fields_verbatim() {
        let json = GOLDEN_JSON_DEFAULTS
            .replacen(
                r#""project_repositories":[]"#,
                r#""project_repositories":[{"url":"https://github.com/qovery/demo","branch":"main","git_credentials":{"login":"x-access-token","access_token":"","expired_at":"1970-01-01T00:00:00Z"}}]"#,
                1,
            )
            .replacen(
                r#""outputs":[]"#,
                r#""outputs":[{"name":"slack","headers":[{"name":"Content-Type","value":""}],"instructions":""}]"#,
                1,
            );
        let workflow: AgenticWorkflow =
            serde_json::from_str(&json).expect("empty static-field JSON should deserialize");

        let config = workflow.into_domain_config().expect("domain config should build");

        assert_eq!(config.project_repositories[0].git_token.as_deref(), Some(""));
        assert_eq!(config.outputs[0].headers, vec![("Content-Type".to_string(), "".to_string())]);
        assert_eq!(config.outputs[0].instructions, "");
    }
}
