use crate::environment::models;
use crate::environment::models::agentic_workflow::{
    AgenticWorkflowConfig, AgenticWorkflowError, AgenticWorkflowService,
};
use crate::infrastructure::models::build_platform::{
    Build, BuildSource, CUSTOM_FRAGMENT_PLACEHOLDER, DockerfileFragment as BuildDockerfileFragment, Image,
};
use crate::infrastructure::models::cloud_provider::service::Action as DomainAction;
use crate::infrastructure::models::container_registry::{
    ContainerRegistryInfo, DockerRegistryInfo, InteractWithRegistry,
};
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::context::Context;
use crate::io_models::models::{CpuArchitecture, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::io_models::services_common::GitCredentials;
use crate::io_models::{Action, QoveryIdentifier, sanitized_git_url};
use crate::utilities::to_short_id;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

/// An agentic workflow carries no build advanced settings on the wire, so these mirror the defaults
/// every other buildable service ships with (see `io_models::job::JobAdvancedSettings::default`).
const BUILD_TIMEOUT_MAX_SEC: u64 = 30 * 60;
const BUILD_CPU_MAX_IN_MILLI: u32 = 4000;
const BUILD_RAM_MAX_IN_GIB: u32 = 8;

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

/// Pinned agent base image (`qovery-ai-runner`), manually pushed to the shared registry (§8.4).
/// Used when the wire payload omits `image`, so the engine has a concrete default even if q-core
/// doesn't dictate one. Bump the tag here (and q-core's `AGENTIC_WORKFLOW_IMAGE_TAG`) when a new
/// image is published. This stays the base a `docker_fragment` is layered onto, so bumping it also
/// invalidates every built per-workflow image.
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
    /// Extra Dockerfile instructions layered onto `image`. When non-empty the engine builds a
    /// per-workflow image from it (see [`AgenticWorkflow::to_build`]) and the Job runs that image
    /// instead of `image`. Empty means no build at all: the Job runs `image` as-is.
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
        default_container_registry: &dyn InteractWithRegistry,
        cluster: &dyn Kubernetes,
    ) -> Result<Box<dyn AgenticWorkflowService>, AgenticWorkflowError> {
        let long_id = self.long_id;
        let name = self.name.clone();
        let kube_name = self.kube_name.clone();
        let action = self.action;
        let build = self.to_build(
            default_container_registry.registry_info(),
            &QoveryIdentifier::new(*cluster.long_id()),
            cluster.cpu_architectures(),
        )?;
        let config = self.into_domain_config()?;

        let service = models::agentic_workflow::AgenticWorkflow::new(
            context,
            long_id,
            name,
            kube_name,
            config,
            build,
            DomainAction::from(action),
            |transmitter| context.get_event_details(transmitter),
        )?;

        Ok(Box::new(service))
    }

    /// A per-workflow image layered on top of the base `image`, or `None` when there is no
    /// `docker_fragment` to layer: then the Job runs the base image directly and nothing is built.
    ///
    /// The base `repository:tag` is written into the `FROM` line rather than passed as a build ARG
    /// so that it is part of the hashed Dockerfile content. Bumping the base tag then produces a new
    /// image tag, instead of the build being skipped as already-present in the registry.
    fn to_build(
        &self,
        cr_info: &ContainerRegistryInfo,
        cluster_id: &QoveryIdentifier,
        architectures: Vec<CpuArchitecture>,
    ) -> Result<Option<Build>, AgenticWorkflowError> {
        let Some(content) = self.dockerfile_content()? else {
            return Ok(None);
        };

        let mut build = Build {
            source: BuildSource::Dockerfile { content },
            image: self.to_image(cr_info, cluster_id),
            environment_variables: BTreeMap::new(),
            disable_buildkit_cache: false,
            timeout: Duration::from_secs(BUILD_TIMEOUT_MAX_SEC),
            architectures,
            max_cpu_in_milli: BUILD_CPU_MAX_IN_MILLI,
            max_ram_in_gib: BUILD_RAM_MAX_IN_GIB,
            ephemeral_storage_in_gib: None,
            registries: vec![],
            dockerfile_fragment: Some(BuildDockerfileFragment::Inline {
                content: self.docker_fragment.clone(),
            }),
        };
        build.compute_image_tag();

        Ok(Some(build))
    }

    /// The whole Dockerfile to build: the base image plus the placeholder the build platform splices
    /// the fragment into. `USER root` comes first so fragments installing packages work, which is
    /// the common case; `USER qovery` is restored afterward so the runner keeps the base image's
    /// runtime user.
    ///
    /// `None` when there is no fragment, which is what makes the whole build path opt-in.
    fn dockerfile_content(&self) -> Result<Option<String>, AgenticWorkflowError> {
        if self.docker_fragment.trim().is_empty() {
            return Ok(None);
        }

        // A fragment declaring its own `FROM` would discard the base image and build something else
        // entirely, which is never what layering onto the agent runner means.
        let first_instruction = self
            .docker_fragment
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or_default();
        if first_instruction
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .eq_ignore_ascii_case("FROM")
        {
            return Err(AgenticWorkflowError::InvalidConfig(
                "docker fragment must not start with a `FROM` instruction: it is layered on top of \
                 the agent base image, it is not a Dockerfile of its own"
                    .to_string(),
            ));
        }

        Ok(Some(format!(
            "FROM {}:{}\nUSER root\n{CUSTOM_FRAGMENT_PLACEHOLDER}\nUSER qovery\n",
            self.image.repository, self.image.tag
        )))
    }

    /// Mirrors [`crate::io_models::job::Job::to_image`], minus the shared-image feature: workflows
    /// have no source repository to share a build across, so each one gets its own repository keyed
    /// on its id. The shared names still have to be filled in, and are derived from a synthetic
    /// identity standing in for the git url the other services use.
    fn to_image(&self, cr_info: &ContainerRegistryInfo, cluster_id: &QoveryIdentifier) -> Image {
        let identity = sanitized_git_url(&format!("agentic-workflow-{}", self.long_id));
        let repository_name = cr_info.get_repository_name(&self.long_id.to_string());
        let image_name = cr_info.get_image_name(&self.long_id.to_string());

        Image {
            service_id: to_short_id(&self.long_id),
            service_long_id: self.long_id,
            service_name: self.name.clone(),
            name: image_name.clone(),
            tag: "".to_string(), // computed by `Build::compute_image_tag` once the build is assembled
            commit_id: "".to_string(),
            registry_name: cr_info.registry_name.clone(),
            registry_url: cr_info.get_registry_endpoint(Some(cluster_id.qovery_resource_name())),
            registry_insecure: cr_info.insecure_registry,
            registry_docker_json_config: cr_info.get_registry_docker_json_config(DockerRegistryInfo {
                registry_name: Some(cr_info.registry_name.to_string()),
                repository_name: Some(repository_name.to_string()),
                image_name: Some(image_name),
            }),
            repository_name,
            shared_repository_name: cr_info.get_shared_repository_name(cluster_id, identity),
            shared_image_feature_enabled: false,
        }
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

    fn workflow_with_fragment(docker_fragment: &str) -> AgenticWorkflow {
        let mut workflow: AgenticWorkflow = serde_json::from_str(GOLDEN_JSON).expect("golden JSON should deserialize");
        workflow.docker_fragment = docker_fragment.to_string();
        workflow
    }

    /// No fragment means nothing to layer, so no build at all: the Job keeps running the base image
    /// straight from its public registry, exactly as before this feature existed.
    #[test]
    fn no_dockerfile_is_generated_without_a_docker_fragment() {
        for fragment in ["", "   ", "\n\t \n"] {
            let content = workflow_with_fragment(fragment)
                .dockerfile_content()
                .expect("an empty fragment is valid");

            assert!(content.is_none(), "fragment {fragment:?} should not produce a Dockerfile");
        }
    }

    #[test]
    fn generated_dockerfile_restores_the_base_image_user_after_the_fragment() {
        let content = workflow_with_fragment("RUN apt-get install -y jq")
            .dockerfile_content()
            .expect("fragment is valid")
            .expect("a fragment produces a Dockerfile");

        assert_eq!(
            content,
            "FROM public.ecr.aws/r3m4q3r9/qovery-ai-runner:0.0.1\nUSER root\n#{{custom_fragment}}\nUSER qovery\n"
        );
    }

    /// The build platform refuses a Dockerfile without the placeholder, so its absence here would
    /// turn every fragment into a build failure.
    #[test]
    fn generated_dockerfile_carries_the_fragment_placeholder() {
        let content = workflow_with_fragment("RUN true")
            .dockerfile_content()
            .expect("fragment is valid")
            .expect("a fragment produces a Dockerfile");

        assert!(content.contains(CUSTOM_FRAGMENT_PLACEHOLDER), "got:\n{content}");
    }

    /// A fragment bringing its own `FROM` would silently replace the agent runner with an unrelated
    /// image, so it is rejected rather than built.
    #[test]
    fn a_fragment_declaring_its_own_from_is_rejected() {
        for fragment in [
            "FROM alpine:3.22",
            "from alpine:3.22\nRUN true",
            "\n# a comment first\n  FROM alpine:3.22",
        ] {
            let error = workflow_with_fragment(fragment)
                .dockerfile_content()
                .expect_err("a fragment starting with FROM must be rejected");

            assert!(
                matches!(&error, AgenticWorkflowError::InvalidConfig(msg) if msg.contains("FROM")),
                "fragment {fragment:?} gave {error:?}"
            );
        }
    }

    /// `FROM` only matters as the leading instruction: a multi-stage `COPY --from` or a later `FROM`
    /// inside a fragment is the author's business.
    #[test]
    fn from_elsewhere_in_a_fragment_is_allowed() {
        let content = workflow_with_fragment("COPY --from=builder /app /app\nRUN chmod +x /app")
            .dockerfile_content()
            .expect("fragment is valid");

        assert!(content.is_some());
    }

    fn image_tag_for(base_tag: &str, fragment: &str) -> String {
        let workflow = {
            let mut workflow = workflow_with_fragment(fragment);
            workflow.image.tag = base_tag.to_string();
            workflow
        };
        let content = workflow
            .dockerfile_content()
            .expect("fragment is valid")
            .expect("a fragment produces a Dockerfile");

        let mut build = Build {
            source: BuildSource::Dockerfile { content },
            image: Image::default(),
            environment_variables: BTreeMap::new(),
            disable_buildkit_cache: false,
            timeout: Duration::from_secs(BUILD_TIMEOUT_MAX_SEC),
            architectures: vec![CpuArchitecture::AMD64],
            max_cpu_in_milli: BUILD_CPU_MAX_IN_MILLI,
            max_ram_in_gib: BUILD_RAM_MAX_IN_GIB,
            ephemeral_storage_in_gib: None,
            registries: vec![],
            dockerfile_fragment: Some(BuildDockerfileFragment::Inline {
                content: workflow.docker_fragment.clone(),
            }),
        };
        build.compute_image_tag();

        build.image.tag
    }

    /// The build is skipped when the target tag already exists in the registry, so both the base tag
    /// and the fragment have to move the tag. Passing the base image as a build ARG instead of
    /// writing it into the `FROM` would break the first half of this.
    #[test]
    fn image_tag_changes_with_the_base_tag_and_with_the_fragment() {
        let baseline = image_tag_for("0.0.1", "RUN apt-get install -y jq");

        assert_ne!(
            baseline,
            image_tag_for("0.0.2", "RUN apt-get install -y jq"),
            "bumping the base image tag must invalidate the built image"
        );
        assert_ne!(
            baseline,
            image_tag_for("0.0.1", "RUN apt-get install -y jq curl"),
            "changing the fragment must invalidate the built image"
        );
        assert_eq!(
            baseline,
            image_tag_for("0.0.1", "RUN apt-get install -y jq"),
            "an unchanged workflow must reuse its image"
        );
    }
}
