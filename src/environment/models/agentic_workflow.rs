use crate::environment::action::DeploymentAction;
use crate::environment::models::types::ToTeraContext;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::Build;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType};
use crate::io_models::agentic_workflow::AgenticWorkflowModelType;
use crate::io_models::context::Context;
use crate::io_models::models::{EnvironmentVariable, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::utilities::to_short_id;
use base64::Engine;
use base64::engine::general_purpose;
use serde::Serialize;
use std::path::PathBuf;
use tera::Context as TeraContext;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum AgenticWorkflowError {
    #[error("AgenticWorkflow invalid configuration: {0}")]
    InvalidConfig(String),
}

/// A single project git repository to be cloned by the agent, mirroring q-job's git-source
/// handling: q-core resolves `gitTokenId` into a short-lived token before it reaches the
/// engine, so the domain layer only ever sees the resolved token value (or `None`).
#[derive(Clone, Debug)]
pub struct AgenticWorkflowProjectRepository {
    pub url: String,
    pub branch: String,
    pub git_token: Option<String>,
}

/// A configured delivery sink for the agent's result.
#[derive(Clone, Debug)]
pub struct AgenticWorkflowOutput {
    pub name: String,
    pub url: Option<String>,
    pub headers: Vec<(String, String)>,
    pub instructions: String,
}

/// Extra configuration beyond `long_id/name/kube_name`, bundled into a single struct so that
/// `AgenticWorkflow::new` stays readable despite the growing field count.
#[derive(Clone, Debug)]
pub struct AgenticWorkflowConfig {
    pub image_repository: String,
    pub image_tag: String,
    /// Extra Dockerfile fragment (§8.3). Not consumed yet - see the comment on
    /// `io_models::agentic_workflow::AgenticWorkflow::docker_fragment`.
    pub docker_fragment: String,
    pub prompt: String,
    pub model_type: AgenticWorkflowModelType,
    pub model_api_key: String,
    pub model_settings: String,
    pub mcp: String,
    pub project_repositories: Vec<AgenticWorkflowProjectRepository>,
    pub host_allowlist: Vec<String>,
    pub outputs: Vec<AgenticWorkflowOutput>,
    pub cpu_request_in_milli: KubernetesCpuResourceUnit,
    pub cpu_limit_in_milli: Option<KubernetesCpuResourceUnit>,
    pub ram_request_in_mib: KubernetesMemoryResourceUnit,
    pub ram_limit_in_mib: KubernetesMemoryResourceUnit,
    pub output_variable_validation_pattern: String,
    pub max_duration_in_sec: u64,
}

impl AgenticWorkflowConfig {
    fn image_full(&self) -> String {
        format!("{}:{}", self.image_repository, self.image_tag)
    }
}

/// Deployment of an AgenticWorkflow: renders and installs a single Kubernetes `Job` running the
/// agent (`qovery-ai-runner`) image, with a `qovery-job-output-waiter` sidecar for output
/// capture. It is intentionally a single concrete struct (not generic over `CloudProvider`)
/// because it renders identical output for every cloud provider.
pub struct AgenticWorkflow {
    pub(crate) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) deployment_id: String,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) action: Action,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) lib_root_directory: String,
    pub(crate) config: AgenticWorkflowConfig,
}

impl AgenticWorkflow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        config: AgenticWorkflowConfig,
        action: Action,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, AgenticWorkflowError> {
        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("agentic_workflows/{long_id}"),
        )
        .map_err(|err| AgenticWorkflowError::InvalidConfig(format!("Can't create workspace directory: {err}")))?;

        // Reuse Transmitter::Job on purpose: q-core's engine-side status handling (a separate
        // Kotlin-side team) already knows how to process Job transmitters, and AgenticWorkflow
        // does not warrant a dedicated Transmitter variant.
        let event_details = mk_event_details(Transmitter::Job(long_id, name.clone()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);

        Ok(Self {
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            deployment_id: context
                .execution_id()
                .rsplit_once('-')
                .map(|s| s.0.to_string())
                .unwrap_or_default(),
            name,
            kube_name,
            action,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
            config,
        })
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn action(&self) -> &Action {
        &self.action
    }

    pub fn workspace_directory(&self) -> &str {
        self.workspace_directory.to_str().unwrap_or("")
    }

    pub fn helm_release_name(&self) -> String {
        self.kube_name.clone()
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-agentic-workflow", self.lib_root_directory)
    }

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub fn output_variable_validation_pattern(&self) -> &str {
        self.config.output_variable_validation_pattern.as_str()
    }

    pub fn max_duration_in_sec(&self) -> u64 {
        self.config.max_duration_in_sec
    }

    pub(crate) fn default_tera_context(&self, target: &DeploymentTarget) -> AgenticWorkflowTeraContext {
        AgenticWorkflowTeraContext {
            namespace: target.environment.namespace().to_string(),
            project_long_id: target.environment.project_long_id,
            environment_long_id: target.environment.long_id,
            deployment_id: self.deployment_id.clone(),
            service: ServiceTeraContext {
                short_id: to_short_id(&self.long_id),
                long_id: self.long_id,
                name: self.name.clone(),
                kube_name: self.kube_name.clone(),
                image_full: self.config.image_full(),
                prompt_b64: general_purpose::STANDARD.encode(self.config.prompt.as_bytes()),
                cpu_request_in_milli: self.config.cpu_request_in_milli.to_string(),
                cpu_limit_in_milli: self.config.cpu_limit_in_milli.as_ref().map(|c| c.to_string()),
                ram_request_in_mib: self.config.ram_request_in_mib.to_string(),
                ram_limit_in_mib: self.config.ram_limit_in_mib.to_string(),
                max_duration_in_sec: self.config.max_duration_in_sec,
            },
            environment_variables: self.get_environment_variables(),
        }
    }
}

impl Service for AgenticWorkflow {
    fn service_type(&self) -> ServiceType {
        ServiceType::AgenticWorkflow
    }

    fn id(&self) -> &str {
        self.id()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name()
    }

    fn version(&self) -> String {
        String::new()
    }

    fn kube_name(&self) -> &str {
        &self.kube_name
    }

    fn kube_label_selector(&self) -> String {
        self.kube_label_selector()
    }

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        self.action()
    }

    fn as_service(&self) -> &dyn Service {
        self
    }

    fn as_service_mut(&mut self) -> &mut dyn Service {
        self
    }

    fn build(&self) -> Option<&Build> {
        // First cut runs the manually-pushed base image (`public.ecr.aws/r3m4q3r9/qovery-ai-runner`);
        // `config.docker_fragment` is threaded through end-to-end but not consumed yet.
        //
        // TODO(QOV-2086, §8.3): build a per-workflow image so `docker_fragment` takes effect.
        // High-level spec:
        //   1. Hold an `Option<Build>` on the domain struct, computed at `new()`, so this can
        //      return a reference (mirror how `io_models/job.rs` constructs a `Build` for
        //      container-source services).
        //   2. Construct that `Build` from the `qovery-ai-runner` source repo:
        //        - `git_repository`: qovery-ai-runner at a pinned ref + its Dockerfile path.
        //        - `image`: target tag in a Qovery registry (call `compute_image_tag()` — it
        //          already factors `dockerfile_fragment` into the tag, so per-fragment cache
        //          keys work for free).
        //        - `dockerfile_fragment: Some(DockerfileFragment::Inline { content:
        //          self.config.docker_fragment.clone() })` — the build platform splices this
        //          into the Dockerfile at build time. NB: this makes the ai-runner repo's own
        //          `render-fragment.sh`/marker mechanism redundant for the engine path (it
        //          stays useful only for manual/local builds).
        //        - resources/registries/architectures as the other services set them.
        //   3. Returning `Some(build)` makes the deploy pipeline build+push before deploy;
        //      point the chart's Job image at the built tag instead of the fixed base image.
        //   4. `build_mut()` must return the same `Build` (the pipeline mutates the tag).
        // Leave `docker_fragment` empty → keep running the fixed base image (skip the build).
        None
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        None
    }

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        let b64 = |value: &str| general_purpose::STANDARD.encode(value.as_bytes());
        let mut vars = vec![
            EnvironmentVariable {
                key: "CLAUDE_MODEL".to_string(),
                value: b64(self.config.model_type.as_engine_str()),
                is_secret: false,
            },
            EnvironmentVariable {
                key: "MODEL_SETTINGS".to_string(),
                value: b64(&self.config.model_settings),
                is_secret: false,
            },
            EnvironmentVariable {
                key: "ANTHROPIC_API_KEY".to_string(),
                value: b64(&self.config.model_api_key),
                is_secret: true,
            },
            EnvironmentVariable {
                key: "MCP_SERVERS".to_string(),
                value: b64(&self.config.mcp),
                is_secret: true,
            },
            EnvironmentVariable {
                key: "HOST_ALLOWLIST".to_string(),
                value: b64(&self.config.host_allowlist.join(",")),
                is_secret: false,
            },
        ];

        if !self.config.project_repositories.is_empty() {
            let repos = self
                .config
                .project_repositories
                .iter()
                .map(|repo| serde_json::json!({"url": repo.url, "branch": repo.branch}))
                .collect::<Vec<_>>();
            let tokens = self
                .config
                .project_repositories
                .iter()
                .map(|repo| repo.git_token.clone().unwrap_or_default())
                .collect::<Vec<_>>();

            vars.push(EnvironmentVariable {
                key: "GIT_REPOS".to_string(),
                value: b64(&serde_json::to_string(&repos).unwrap_or_else(|_| "[]".to_string())),
                is_secret: false,
            });
            vars.push(EnvironmentVariable {
                key: "GIT_TOKENS".to_string(),
                value: b64(&serde_json::to_string(&tokens).unwrap_or_else(|_| "[]".to_string())),
                is_secret: true,
            });
        }

        if !self.config.outputs.is_empty() {
            let outputs = self
                .config
                .outputs
                .iter()
                .map(|output| {
                    serde_json::json!({
                        "name": output.name,
                        "url": output.url,
                        "headers": output.headers.iter().map(|(name, value)| serde_json::json!({"name": name, "value": value})).collect::<Vec<_>>(),
                        "instructions": output.instructions,
                    })
                })
                .collect::<Vec<_>>();

            vars.push(EnvironmentVariable {
                key: "OUTPUTS".to_string(),
                value: b64(&serde_json::to_string(&outputs).unwrap_or_else(|_| "[]".to_string())),
                is_secret: true,
            });
        }

        vars
    }
}

impl ToTeraContext for AgenticWorkflow {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        Ok(TeraContext::from_serialize(self.default_tera_context(target)).unwrap_or_default())
    }
}

pub trait AgenticWorkflowService: Service + DeploymentAction + ToTeraContext + Send {
    fn as_deployment_action(&self) -> &dyn DeploymentAction;
    fn output_variable_validation_pattern(&self) -> &str;
    fn max_duration_in_sec(&self) -> u64;
}

impl AgenticWorkflowService for AgenticWorkflow
where
    AgenticWorkflow: Service + DeploymentAction + ToTeraContext,
{
    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }

    fn output_variable_validation_pattern(&self) -> &str {
        self.output_variable_validation_pattern()
    }

    fn max_duration_in_sec(&self) -> u64 {
        self.max_duration_in_sec()
    }
}

impl AgenticWorkflowModelType {
    /// The literal string rendered into the `CLAUDE_MODEL` env var, matching q-core's
    /// `AgenticWorkflowModelType` enum constant names.
    fn as_engine_str(&self) -> &'static str {
        match self {
            AgenticWorkflowModelType::Claude => "CLAUDE",
            AgenticWorkflowModelType::Bedrock => "BEDROCK",
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct ServiceTeraContext {
    pub(crate) short_id: String,
    pub(crate) long_id: Uuid,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) image_full: String,
    /// Base64-encoded prompt content, rendered into a ConfigMap `binaryData` key so arbitrary
    /// prompt text never needs YAML-safe escaping.
    pub(crate) prompt_b64: String,
    pub(crate) cpu_request_in_milli: String,
    pub(crate) cpu_limit_in_milli: Option<String>,
    pub(crate) ram_request_in_mib: String,
    pub(crate) ram_limit_in_mib: String,
    pub(crate) max_duration_in_sec: u64,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct AgenticWorkflowTeraContext {
    pub(crate) namespace: String,
    pub(crate) project_long_id: Uuid,
    pub(crate) environment_long_id: Uuid,
    pub(crate) deployment_id: String,
    pub(crate) service: ServiceTeraContext,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
}

#[cfg(test)]
mod tests {
    use super::{AgenticWorkflowTeraContext, ServiceTeraContext};
    use crate::io_models::models::EnvironmentVariable;
    use base64::Engine;
    use base64::engine::general_purpose;
    use tera::{Context, Tera};
    use uuid::Uuid;

    fn render_template(template: &str, context: AgenticWorkflowTeraContext) -> String {
        let tera_context = Context::from_serialize(context).expect("agentic workflow tera context should serialize");
        Tera::one_off(template, &tera_context, false).expect("template should render")
    }

    fn build_agentic_workflow_tera_context() -> AgenticWorkflowTeraContext {
        AgenticWorkflowTeraContext {
            namespace: "test-namespace".to_string(),
            project_long_id: Uuid::new_v4(),
            environment_long_id: Uuid::new_v4(),
            deployment_id: "test-deployment-id".to_string(),
            service: ServiceTeraContext {
                short_id: "zabc12345".to_string(),
                long_id: Uuid::new_v4(),
                name: "test agentic workflow".to_string(),
                kube_name: "test-agentic-workflow".to_string(),
                image_full: "public.ecr.aws/r3m4q3r9/qovery-ai-runner:1.0.0".to_string(),
                prompt_b64: "aGVsbG8gd29ybGQ=".to_string(),
                cpu_request_in_milli: "500m".to_string(),
                cpu_limit_in_milli: Some("1000m".to_string()),
                ram_request_in_mib: "512Mi".to_string(),
                ram_limit_in_mib: "1024Mi".to_string(),
                max_duration_in_sec: 3_600,
            },
            environment_variables: vec![
                EnvironmentVariable {
                    key: "ANTHROPIC_API_KEY".to_string(),
                    value: "c2VjcmV0".to_string(),
                    is_secret: true,
                },
                EnvironmentVariable {
                    key: "CLAUDE_MODEL".to_string(),
                    value: "Q0xBVURF".to_string(),
                    is_secret: false,
                },
            ],
        }
    }

    fn job_template() -> &'static str {
        include_str!("../../../lib/common/charts/q-agentic-workflow/templates/job.j2.yaml")
    }

    #[test]
    fn renders_job_template_with_expected_content() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("kind: Job"));
        assert!(rendered.contains("apiVersion: batch/v1"));
        assert!(rendered.contains("test-namespace"));
        assert!(rendered.contains("qovery.com/service-id"));
        assert!(rendered.contains("qovery.com/environment-id"));
        assert!(rendered.contains("qovery.com/project-id"));
        assert!(rendered.contains("qovery.com/deployment-id"));
        assert!(rendered.contains("test-deployment-id"));
    }

    #[test]
    fn renders_job_template_using_kube_name_not_unsafe_name() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("name: test-agentic-workflow"));
        assert!(!rendered.contains("test agentic workflow"));
    }

    #[test]
    fn renders_job_template_with_agent_image_instead_of_busybox() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("public.ecr.aws/r3m4q3r9/qovery-ai-runner:1.0.0"));
        assert!(!rendered.contains("busybox"));
        assert!(!rendered.contains("hello world"));
    }

    #[test]
    fn renders_job_template_with_output_waiter_sidecar_and_shared_volume() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("qovery-wait-container-output"));
        assert!(rendered.contains("public.ecr.aws/r3m4q3r9/qovery-job-output-waiter"));
        assert!(rendered.contains("qovery-job-output-waiter\", \"--watch\""));
        assert!(rendered.contains("restartPolicy: Always"));
        assert!(rendered.contains("/qovery-output"));
        assert!(rendered.contains("name: output"));
    }

    #[test]
    fn renders_job_template_with_prompt_configmap_mount_and_envfrom_secret() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("PROMPT_FILE"));
        assert!(rendered.contains("prompt.txt"));
        assert!(rendered.contains("envFrom"));
        assert!(rendered.contains("secretRef"));
        assert!(rendered.contains("name: test-agentic-workflow"));
    }

    #[test]
    fn renders_job_template_in_oneshot_mode_not_server() {
        // A Job must run the agent once and exit. The ai-runner defaults to `server` (long-lived
        // HTTP server that never processes the prompt), so the template must force oneshot.
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("name: RUN_MODE"));
        assert!(rendered.contains("value: \"oneshot\""));
    }

    #[test]
    fn renders_job_template_with_configured_resources_and_deadline() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("activeDeadlineSeconds: 3600"));
        assert!(rendered.contains("cpu: 500m"));
        assert!(rendered.contains("cpu: 1000m"));
        assert!(rendered.contains("memory: 512Mi"));
        assert!(rendered.contains("memory: 1024Mi"));
    }

    fn secret_template() -> &'static str {
        include_str!("../../../lib/common/charts/q-agentic-workflow/templates/secret.j2.yaml")
    }

    #[test]
    fn renders_secret_template_with_injected_config() {
        let rendered = render_template(secret_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("kind: Secret"));
        assert!(rendered.contains("name: test-agentic-workflow"));
        assert!(rendered.contains("ANTHROPIC_API_KEY"));
        assert!(rendered.contains("c2VjcmV0"));
        assert!(rendered.contains("CLAUDE_MODEL"));
    }

    /// Regression guard for QOV-2086: the secret MUST use `stringData`, not `data`.
    ///
    /// The engine base64-encodes every env-var value (`get_environment_variables`) and the
    /// container base64-decodes it (`ai-runner/src/engine_env.rs`). `stringData` stores the value
    /// verbatim, so the container receives exactly the engine's single-encoded value and its
    /// decode recovers the canonical content. A `data:` field would make Kubernetes decode the
    /// value a second time at `envFrom` injection, so the container would receive plaintext and
    /// its decode would fail with "Invalid padding". This test models that one-layer contract:
    /// the rendered value is the engine's `base64("CLAUDE")`, and decoding it once yields "CLAUDE".
    #[test]
    fn secret_uses_string_data_so_container_decode_recovers_canonical_value() {
        let rendered = render_template(secret_template(), build_agentic_workflow_tera_context());

        // Transport must be `stringData` (verbatim), never `data` (which k8s would decode).
        assert!(rendered.contains("stringData:"), "secret must use stringData, got:\n{rendered}");
        assert!(
            !rendered.contains("\ndata:"),
            "secret must not use a `data:` field:\n{rendered}"
        );

        // The fixture's CLAUDE_MODEL value is the engine's emitted `base64("CLAUDE")`. With
        // stringData it reaches the container verbatim; the container decodes it exactly once.
        assert!(rendered.contains("Q0xBVURF"));
        let container_env_value = "Q0xBVURF"; // what `envFrom` injects verbatim under stringData
        let decoded_once = String::from_utf8(
            general_purpose::STANDARD
                .decode(container_env_value)
                .expect("container's single base64 decode must succeed"),
        )
        .expect("decoded value must be valid UTF-8");
        assert_eq!(decoded_once, "CLAUDE");
    }

    fn prompt_config_map_template() -> &'static str {
        include_str!("../../../lib/common/charts/q-agentic-workflow/templates/prompt_config_map.j2.yaml")
    }

    #[test]
    fn renders_prompt_config_map_with_base64_prompt() {
        let rendered = render_template(prompt_config_map_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("kind: ConfigMap"));
        assert!(rendered.contains("name: test-agentic-workflow-prompt"));
        assert!(rendered.contains("binaryData"));
        assert!(rendered.contains("aGVsbG8gd29ybGQ="));
    }
}
