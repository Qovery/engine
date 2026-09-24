use crate::environment::action::DeploymentAction;
use crate::environment::models::container::RegistryTeraContext;
use crate::environment::models::types::ToTeraContext;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::Build;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType};
use crate::infrastructure::models::container_registry::DockerRegistryInfo;
use crate::io_models::agentic_workflow::AgenticWorkflowModelType;
use crate::io_models::context::Context;
use crate::io_models::models::{
    EnvironmentVariable, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit, MountedFile,
};
use crate::io_models::variable_utils::VariableInfo;
use crate::utilities::to_short_id;
use base64::Engine;
use base64::engine::general_purpose;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tera::Context as TeraContext;
use tracing::warn;
use uuid::Uuid;

/// User variables cannot shadow engine contract or runner environment names.
/// Keep this list in the engine because it describes the chart/ai-runner contract.
// Bedrock-only chart variables stay available to Claude workflows' user AWS environment.
// TODO: Reserve the entire `INPUT_` prefix, not just `INPUTS_FILE`; ai-runner turns every matching
// process variable into prompt input, which can expose a secret user variable to the LLM.
const RESERVED_ENVIRONMENT_VARIABLE_NAMES: &[&str] = &[
    "CLAUDE_MODEL",
    "MODEL_SETTINGS",
    "ANTHROPIC_API_KEY",
    "MCP_SERVERS",
    "HOST_ALLOWLIST",
    "GIT_REPOS",
    "GIT_TOKENS",
    "OUTPUTS",
    "RUN_MODE",
    "PROMPT_FILE",
    "INPUTS_FILE",
];

/// Bedrock-only names in the user Secret block; collisions would create duplicate YAML keys.
// Claude workflows keep these names available for their own AWS environment.
const BEDROCK_RESERVED_ENVIRONMENT_VARIABLE_NAMES: &[&str] = &[
    "AWS_BEARER_TOKEN_BEDROCK",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
];

#[derive(thiserror::Error, Debug)]
pub enum AgenticWorkflowError {
    #[error("AgenticWorkflow invalid configuration: {0}")]
    InvalidConfig(String),
}

/// A single project git repository to be cloned by the agent, mirroring q-job's git-source
/// handling: q-core resolves `gitTokenId` into a short-lived token before it reaches the
/// engine, so the domain layer only ever sees the resolved token value (or `None`).
#[derive(Clone)]
pub struct AgenticWorkflowProjectRepository {
    pub url: String,
    pub branch: String,
    pub git_token: Option<String>,
}

impl std::fmt::Debug for AgenticWorkflowProjectRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticWorkflowProjectRepository")
            .field("url", &self.url)
            .field("branch", &self.branch)
            .field("git_token", &self.git_token.as_ref().map(|_| "***"))
            .finish()
    }
}

/// A configured delivery sink for the agent's result.
#[derive(Clone)]
pub struct AgenticWorkflowOutput {
    pub name: String,
    pub url: Option<String>,
    pub headers: Vec<(String, String)>,
    pub instructions: String,
}

impl std::fmt::Debug for AgenticWorkflowOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticWorkflowOutput")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("header_count", &self.headers.len())
            .field("instructions_length", &self.instructions.len())
            .finish()
    }
}

/// Repeated header names are kept: collapsing is a rendering concern, handled in
/// [`run_inputs_json`].
#[derive(Clone)]
pub struct AgenticWorkflowRunPayload {
    pub body: String,
    pub headers: Vec<(String, String)>,
}

impl std::fmt::Debug for AgenticWorkflowRunPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticWorkflowRunPayload")
            .field("body_length", &self.body.len())
            .field("header_count", &self.headers.len())
            .finish()
    }
}

const WEBHOOK_BODY_INPUT_KEY: &str = "WEBHOOK_BODY";
const WEBHOOK_HEADERS_INPUT_KEY: &str = "WEBHOOK_HEADERS";

/// Renders the shape `ai-runner` requires at `INPUTS_FILE`: a flat object of `string -> string`,
/// which it folds into the prompt as `{{WEBHOOK_BODY}}` / `{{WEBHOOK_HEADERS}}` substitutions plus
/// a `# INPUTS` section (`ai-runner/src/prompt_builder.rs`). Headers are a serialized JSON *string*
/// because a nested object would not parse as `HashMap<String, String>` there.
///
/// `"{}"` when there is no payload, never empty: the file is always mounted and `load_inputs`
/// treats an unparseable one as fatal.
///
/// `BTreeMap` for deterministic key order, so an unchanged workflow does not churn its Helm release.
fn run_inputs_json(payload: &Option<AgenticWorkflowRunPayload>) -> String {
    let Some(payload) = payload else {
        return "{}".to_string();
    };

    let mut headers: BTreeMap<&str, String> = BTreeMap::new();
    for (name, value) in &payload.headers {
        headers
            .entry(name.as_str())
            .and_modify(|existing| {
                // RFC 9110 §5.3: repeated field names are equivalent to one comma-joined value.
                existing.push_str(", ");
                existing.push_str(value);
            })
            .or_insert_with(|| value.clone());
    }

    let mut inputs: BTreeMap<&str, String> = BTreeMap::new();
    inputs.insert(WEBHOOK_BODY_INPUT_KEY, payload.body.clone());
    inputs.insert(
        WEBHOOK_HEADERS_INPUT_KEY,
        serde_json::to_string(&headers).unwrap_or_else(|_| "{}".to_string()),
    );

    serde_json::to_string(&inputs).unwrap_or_else(|_| "{}".to_string())
}

/// Bedrock authentication. The custom `Debug` implementation redacts its token.
#[derive(Clone, PartialEq, Eq)]
pub enum BedrockAuth {
    /// Bearer token read by the Claude CLI from `AWS_BEARER_TOKEN_BEDROCK`.
    BearerToken(String),
}

impl std::fmt::Debug for BedrockAuth {
    /// Redact the bearer token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BedrockAuth::BearerToken(_) => f.write_str("BearerToken(***)"),
        }
    }
}

/// Region and authentication for a Bedrock run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BedrockRuntime {
    pub region: String,
    pub auth: BedrockAuth,
}

/// Extra configuration beyond `long_id/name/kube_name`, bundled into a single struct so that
/// `AgenticWorkflow::new` stays readable despite the growing field count.
#[derive(Clone)]
pub struct AgenticWorkflowConfig {
    pub image_repository: String,
    pub image_tag: String,
    /// Extra Dockerfile instructions layered onto the base image. Turned into the workflow's
    /// [`Build`] at conversion time; kept here as the record of what was asked for.
    pub docker_fragment: String,
    pub prompt: String,
    pub model_type: AgenticWorkflowModelType,
    /// Kubernetes cluster region used as a fallback for legacy Bedrock requests without a model region.
    pub cluster_region: String,
    /// Internal Bedrock runtime settings derived from the flat workflow model, when selected.
    pub bedrock: Option<BedrockRuntime>,
    /// Anthropic key for Claude runs; Bedrock emits no `ANTHROPIC_API_KEY`.
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
    /// Absent for a deploy with no triggering event.
    pub run_payload: Option<AgenticWorkflowRunPayload>,
    /// User-defined variables, plaintext as they arrive from q-core. Encoded on the way into the
    /// Secret's `data:` block, see [`to_user_environment_variables`].
    pub environment_variables: BTreeMap<String, VariableInfo>,
    /// FILE variables, each rendered as its own Secret and mounted into the Job at
    /// `mount_path`. Content arrives already base64-encoded, so nothing here re-encodes it.
    ///
    /// `BTreeSet` for the same reason the maps above are ordered: a stable rendering order keeps an
    /// unchanged workflow from churning its Helm release.
    pub mounted_files: BTreeSet<MountedFile>,
}

impl AgenticWorkflowConfig {
    fn image_full(&self) -> String {
        format!("{}:{}", self.image_repository, self.image_tag)
    }

    /// Build the Bedrock chart context and encoded credentials; `None` for Claude.
    pub(crate) fn bedrock(&self) -> Option<(BedrockTeraContext, Vec<EnvironmentVariable>)> {
        let b64 = |value: &str| general_purpose::STANDARD.encode(value.as_bytes());
        let secret = |key: &str, value: &str| EnvironmentVariable {
            key: key.to_string(),
            value: b64(value),
            is_secret: true,
        };

        if self.model_type != AgenticWorkflowModelType::Bedrock {
            return None;
        }
        let bedrock = self.bedrock.as_ref()?;

        let credentials = match &bedrock.auth {
            BedrockAuth::BearerToken(token) => vec![secret("AWS_BEARER_TOKEN_BEDROCK", token)],
        };

        Some((
            BedrockTeraContext {
                region: bedrock.region.clone(),
            },
            credentials,
        ))
    }

    /// Add Bedrock-only AWS names to the reserved user-variable set.
    fn reserved_environment_variable_names(&self) -> Vec<&'static str> {
        let mut reserved = RESERVED_ENVIRONMENT_VARIABLE_NAMES.to_vec();
        if self.model_type == AgenticWorkflowModelType::Bedrock {
            reserved.extend_from_slice(BEDROCK_RESERVED_ENVIRONMENT_VARIABLE_NAMES);
        }
        reserved
    }
}

/// Encode engine contract variables for the Secret's `stringData` block.
fn contract_environment_variables(config: &AgenticWorkflowConfig) -> Vec<EnvironmentVariable> {
    let b64 = |value: &str| general_purpose::STANDARD.encode(value.as_bytes());
    let mut vars = vec![
        EnvironmentVariable {
            key: "CLAUDE_MODEL".to_string(),
            value: b64(config.model_type.as_engine_str()),
            is_secret: false,
        },
        EnvironmentVariable {
            key: "MODEL_SETTINGS".to_string(),
            value: b64(&config.model_settings),
            is_secret: false,
        },
        EnvironmentVariable {
            key: "MCP_SERVERS".to_string(),
            value: b64(&config.mcp),
            is_secret: true,
        },
        EnvironmentVariable {
            key: "HOST_ALLOWLIST".to_string(),
            value: b64(&config.host_allowlist.join(",")),
            is_secret: false,
        },
    ];

    // Bedrock must never receive an Anthropic key that could reroute the CLI.
    if config.model_type == AgenticWorkflowModelType::Claude {
        vars.insert(
            2,
            EnvironmentVariable {
                key: "ANTHROPIC_API_KEY".to_string(),
                value: b64(&config.model_api_key),
                is_secret: true,
            },
        );
    }

    if !config.project_repositories.is_empty() {
        let repos = config
            .project_repositories
            .iter()
            .map(|repo| serde_json::json!({"url": repo.url, "branch": repo.branch}))
            .collect::<Vec<_>>();
        let tokens = config
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

    if !config.outputs.is_empty() {
        let outputs = config
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

/// Custom `Debug` redacts credentials and opaque fields.
impl std::fmt::Debug for AgenticWorkflowConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticWorkflowConfig")
            .field("image", &self.image_full())
            .field("model_type", &self.model_type)
            .field("cluster_region", &self.cluster_region)
            .field("bedrock", &self.bedrock)
            .field("model_api_key", &"***")
            .field("model_settings", &self.model_settings)
            .field("mcp", &"***")
            .field("project_repositories", &self.project_repositories.len())
            .field("host_allowlist", &self.host_allowlist)
            .field("outputs", &self.outputs.len())
            .field("cpu_request_in_milli", &self.cpu_request_in_milli)
            .field("cpu_limit_in_milli", &self.cpu_limit_in_milli)
            .field("ram_request_in_mib", &self.ram_request_in_mib)
            .field("ram_limit_in_mib", &self.ram_limit_in_mib)
            .field("output_variable_validation_pattern", &self.output_variable_validation_pattern)
            .field("max_duration_in_sec", &self.max_duration_in_sec)
            .field("run_payload", &self.run_payload.is_some())
            .field("environment_variables", &self.environment_variables.len())
            .field("mounted_files", &self.mounted_files.len())
            .finish()
    }
}

/// Kubernetes pull secret rendered by the chart when the Job runs an image built by the engine, and
/// therefore living in the cluster's private registry.
fn registry_tera_context(build: &Build, target: &DeploymentTarget, kube_name: &str) -> Option<RegistryTeraContext> {
    let registry_info = target.container_registry.registry_info();

    registry_info
        .get_registry_docker_json_config(DockerRegistryInfo {
            registry_name: Some(target.kubernetes.cluster_name()),
            repository_name: Some(build.image.repository_name().to_string()),
            image_name: Some(build.image.name()),
        })
        .map(|docker_json| RegistryTeraContext {
            secret_name: format!("{kube_name}-registry"),
            docker_json_config: Some(docker_json),
        })
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
    /// Present only when `config.docker_fragment` asks for extra layers on the base image. `None`
    /// means the Job runs the base image straight from its public registry.
    pub(crate) build: Option<Build>,
}

impl AgenticWorkflow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        config: AgenticWorkflowConfig,
        build: Option<Build>,
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
            build,
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
        let bedrock = self.config.bedrock();
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
                image_full: self
                    .build
                    .as_ref()
                    .map(|build| build.image.full_image_name_with_tag())
                    .unwrap_or_else(|| self.config.image_full()),
                prompt_b64: general_purpose::STANDARD.encode(self.config.prompt.as_bytes()),
                inputs_json_b64: general_purpose::STANDARD.encode(run_inputs_json(&self.config.run_payload).as_bytes()),
                cpu_request_in_milli: self.config.cpu_request_in_milli.to_string(),
                cpu_limit_in_milli: self.config.cpu_limit_in_milli.as_ref().map(|c| c.to_string()),
                ram_request_in_mib: self.config.ram_request_in_mib.to_string(),
                ram_limit_in_mib: self.config.ram_limit_in_mib.to_string(),
                max_duration_in_sec: self.config.max_duration_in_sec,
                bedrock: bedrock.as_ref().map(|(context, _)| context.clone()),
            },
            environment_variables: self.get_environment_variables(),
            bedrock_credentials: bedrock.map(|(_, credentials)| credentials).unwrap_or_default(),
            user_environment_variables: to_user_environment_variables(
                &self.config.environment_variables,
                &self.long_id,
                &self.config.reserved_environment_variable_names(),
            ),
            mounted_files: self.config.mounted_files.iter().cloned().collect::<Vec<_>>(),
            registry: self
                .build
                .as_ref()
                .and_then(|build| registry_tera_context(build, target, self.kube_name.as_str())),
        }
    }
}

/// User-defined variables, base64-encoded for the Secret's `data:` block so Kubernetes hands the
/// container plaintext. Encoding here (rather than in q-core, as every other service does) also
/// keeps arbitrary values — newlines, leading spaces — out of the `stringData` block scalar, where
/// they would break the rendered YAML.
///
/// Names that would collide with something the runner needs are dropped. Kubernetes already makes
/// such a collision harmless — `stringData` wins over `data`, and a pod's `env:` wins over
/// `envFrom` — so the filter exists to tell the user their variable is being ignored, not to make
/// the Secret safe.
///
/// Names that are not plain identifiers are dropped as well. A value is base64 by the time it is
/// rendered, but a key is not, so a newline or a `": "` in one turns the Secret template into a
/// YAML scanner error — which takes the registry pull secret rendered after it down too, and
/// reports a template failure that names no key. This is stricter than the `[-._a-zA-Z0-9]+`
/// Kubernetes accepts for a Secret key, on purpose: a name holding `-` or `.` is not a shell
/// identifier, so `envFrom` drops it anyway and says so only in an event on the pod.
///
/// A free function rather than a method so it can be unit-tested without a `DeploymentTarget`.
// TODO: Both filters only `warn!`, which reaches the engine's own logs and not the deployment logs
// the user reads — those need `Logger::log(EngineEvent::…)` and an `EventDetails`, which nothing in
// this layer holds. A reserved name is documented, so an engine-side line is enough; a name like
// `MY-KEY` is not, and the user currently just gets a variable that does not exist.
fn to_user_environment_variables(
    variables: &BTreeMap<String, VariableInfo>,
    workflow_long_id: &Uuid,
    reserved: &[&str],
) -> Vec<EnvironmentVariable> {
    variables
        .iter()
        .filter(|(key, _)| {
            if reserved.contains(&key.as_str()) {
                warn!(
                    "agentic workflow {workflow_long_id}: ignoring environment variable {key}, the name is reserved by the runner"
                );
                return false;
            }
            if !is_environment_variable_name(key) {
                // Quoted: the key may itself hold a newline.
                warn!(
                    "agentic workflow {workflow_long_id}: ignoring environment variable {key:?}, the name is not a valid identifier"
                );
                return false;
            }
            true
        })
        .map(|(key, variable)| EnvironmentVariable {
            key: key.clone(),
            value: general_purpose::STANDARD.encode(variable.value.as_bytes()),
            is_secret: variable.is_secret,
        })
        .collect()
}

/// `[A-Za-z_][A-Za-z0-9_]*`, spelled out rather than compiled: one charset this small does not
/// warrant a regex on a path walked once per variable per deployment.
fn is_environment_variable_name(key: &str) -> bool {
    let mut chars = key.chars();

    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
        self.build.as_ref()
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        // The build pipeline rewrites the image tag once it knows which Dockerfile args are used,
        // so it must get the very same `Build` the tera context later reads.
        self.build.as_mut()
    }

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        contract_environment_variables(&self.config)
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

#[derive(Serialize, Clone)]
pub(crate) struct ServiceTeraContext {
    pub(crate) short_id: String,
    pub(crate) long_id: Uuid,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) image_full: String,
    /// Base64-encoded prompt content, rendered into a ConfigMap `binaryData` key so arbitrary
    /// prompt text never needs YAML-safe escaping.
    pub(crate) prompt_b64: String,
    /// Base64 like `prompt_b64`, for the same reason: arbitrary external text that must not need
    /// YAML escaping.
    pub(crate) inputs_json_b64: String,
    pub(crate) cpu_request_in_milli: String,
    pub(crate) cpu_limit_in_milli: Option<String>,
    pub(crate) ram_request_in_mib: String,
    pub(crate) ram_limit_in_mib: String,
    pub(crate) max_duration_in_sec: u64,
    /// Bedrock chart context, absent for Claude.
    pub(crate) bedrock: Option<BedrockTeraContext>,
}

impl std::fmt::Debug for ServiceTeraContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceTeraContext")
            .field("short_id", &self.short_id)
            .field("long_id", &self.long_id)
            .field("name", &self.name)
            .field("kube_name", &self.kube_name)
            .field("image_full", &self.image_full)
            .field("prompt", &"***")
            .field("inputs", &"***")
            .field("cpu_request_in_milli", &self.cpu_request_in_milli)
            .field("cpu_limit_in_milli", &self.cpu_limit_in_milli)
            .field("ram_request_in_mib", &self.ram_request_in_mib)
            .field("ram_limit_in_mib", &self.ram_limit_in_mib)
            .field("max_duration_in_sec", &self.max_duration_in_sec)
            .field("bedrock", &self.bedrock)
            .finish()
    }
}

/// Bedrock values rendered into the Job.
#[derive(Serialize, Debug, Clone)]
pub(crate) struct BedrockTeraContext {
    /// `AWS_REGION` value rendered in plaintext.
    pub(crate) region: String,
}

#[derive(Serialize, Clone)]
pub(crate) struct AgenticWorkflowTeraContext {
    pub(crate) namespace: String,
    pub(crate) project_long_id: Uuid,
    pub(crate) environment_long_id: Uuid,
    pub(crate) deployment_id: String,
    pub(crate) service: ServiceTeraContext,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    /// User-defined variables, rendered into the Secret's `data:` block rather than `stringData`.
    pub(crate) user_environment_variables: Vec<EnvironmentVariable>,
    /// Base64-encoded Bedrock credentials for Secret `data`; empty for Claude.
    pub(crate) bedrock_credentials: Vec<EnvironmentVariable>,
    /// One Secret and one volume mount per entry. `Vec` because Tera iterates it; the ordering
    /// comes from the `BTreeSet` it is built from.
    pub(crate) mounted_files: Vec<MountedFile>,
    /// `None` when the Job runs the public base image and needs no pull secret.
    pub(crate) registry: Option<RegistryTeraContext>,
}

impl std::fmt::Debug for AgenticWorkflowTeraContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bedrock_credential_names = self
            .bedrock_credentials
            .iter()
            .map(|credential| credential.key.as_str())
            .collect::<Vec<_>>();

        f.debug_struct("AgenticWorkflowTeraContext")
            .field("namespace", &self.namespace)
            .field("project_long_id", &self.project_long_id)
            .field("environment_long_id", &self.environment_long_id)
            .field("deployment_id", &self.deployment_id)
            .field("service", &self.service)
            .field("environment_variable_count", &self.environment_variables.len())
            .field("user_environment_variable_count", &self.user_environment_variables.len())
            .field("bedrock_credential_names", &bedrock_credential_names)
            .field("mounted_file_count", &self.mounted_files.len())
            .field("has_registry", &self.registry.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgenticWorkflowConfig, AgenticWorkflowRunPayload, AgenticWorkflowTeraContext,
        BEDROCK_RESERVED_ENVIRONMENT_VARIABLE_NAMES, BedrockAuth, BedrockRuntime, BedrockTeraContext,
        RESERVED_ENVIRONMENT_VARIABLE_NAMES, ServiceTeraContext, contract_environment_variables, run_inputs_json,
        to_user_environment_variables,
    };
    use crate::environment::models::container::RegistryTeraContext;
    use crate::io_models::agentic_workflow::AgenticWorkflowModelType;
    use crate::io_models::models::{EnvironmentVariable, MountedFile};
    use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
    use crate::io_models::variable_utils::VariableInfo;
    use crate::tera_utils::render_one_off;
    use base64::Engine;
    use base64::engine::general_purpose;
    use serde::Deserialize;
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use tera::Context;
    use uuid::Uuid;

    fn render_template(template: &str, context: AgenticWorkflowTeraContext) -> String {
        let tera_context = Context::from_serialize(context).expect("agentic workflow tera context should serialize");
        render_one_off(template, &tera_context).expect("template should render")
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
                inputs_json_b64: "eyJXRUJIT09LX0JPRFkiOiJ7XCJpc3N1ZVwiOntcImtleVwiOlwiUU9WLTFcIn19IiwiV0VCSE9PS19IRUFERVJTIjoie1wiY29udGVudC10eXBlXCI6XCJhcHBsaWNhdGlvbi9qc29uXCIsXCJ4LWF0bGFzc2lhbi13ZWJob29rLWlkZW50aWZpZXJcIjpcImFiYy0xMjNcIn0ifQ==".to_string(),
                cpu_request_in_milli: "500m".to_string(),
                cpu_limit_in_milli: Some("1000m".to_string()),
                ram_request_in_mib: "512Mi".to_string(),
                ram_limit_in_mib: "1024Mi".to_string(),
                max_duration_in_sec: 3_600,
                bedrock: None,
            },
            bedrock_credentials: vec![],
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
            user_environment_variables: vec![],
            mounted_files: vec![],
            registry: None,
        }
    }

    /// Same context, but for a workflow whose image the engine built from a docker fragment: it now
    /// lives in the cluster's private registry and needs a pull secret.
    fn build_agentic_workflow_tera_context_with_built_image() -> AgenticWorkflowTeraContext {
        let mut context = build_agentic_workflow_tera_context();
        context.service.image_full = "registry.qovery.com/z1234567:mytag".to_string();
        context.registry = Some(RegistryTeraContext {
            secret_name: "test-agentic-workflow-registry".to_string(),
            docker_json_config: Some("eyJhdXRocyI6e319".to_string()),
        });
        context
    }

    fn job_template() -> &'static str {
        include_str!("../../../lib/common/charts/q-agentic-workflow/templates/job.j2.yaml")
    }

    fn mounted_files_secret_template() -> &'static str {
        include_str!("../../../lib/common/charts/q-agentic-workflow/templates/mounted_files_secret.j2.yaml")
    }

    /// `base64("{\"retries\":3}")`, as q-core sends it.
    const MOUNTED_FILE_CONTENT_B64: &str = "eyJyZXRyaWVzIjozfQ==";

    fn build_context_with_mounted_file() -> AgenticWorkflowTeraContext {
        let mut context = build_agentic_workflow_tera_context();
        context.mounted_files = vec![MountedFile {
            long_id: Uuid::new_v4(),
            kube_name: "config-file-secret".to_string(),
            mount_path: "/etc/config.json".to_string(),
            file_content_b64: MOUNTED_FILE_CONTENT_B64.to_string(),
        }];
        context
    }

    /// The counterpart to `secret_uses_string_data_so_container_decode_recovers_canonical_value`:
    /// a mounted file gets exactly ONE layer of encoding, applied by q-core, and Kubernetes strips
    /// it. Nothing in the container decodes a file, so `data` is required here where the contract
    /// variables need `stringData`. Rendering under `stringData` would leave base64 on disk.
    #[test]
    fn mounted_file_secret_uses_data_so_kubernetes_decode_yields_plaintext_on_disk() {
        let rendered = render_template(mounted_files_secret_template(), build_context_with_mounted_file());

        assert!(rendered.contains("kind: Secret"));
        assert!(rendered.contains("name: config-file-secret"));
        assert!(rendered.contains("data:"));
        assert!(!rendered.contains("stringData:"));
        assert!(rendered.contains(MOUNTED_FILE_CONTENT_B64));

        // What Kubernetes writes to disk is this decoded once, i.e. the original content.
        let decoded = general_purpose::STANDARD
            .decode(MOUNTED_FILE_CONTENT_B64)
            .expect("q-core sends valid base64");
        assert_eq!(String::from_utf8(decoded).unwrap(), r#"{"retries":3}"#);
    }

    #[test]
    fn renders_no_mounted_file_secret_when_the_workflow_has_no_file_variable() {
        let rendered = render_template(mounted_files_secret_template(), build_agentic_workflow_tera_context());

        assert!(!rendered.contains("kind: Secret"));
    }

    /// A file variable is only usable if the volume, the mount and the env var agree on the path.
    /// The env var comes from q-core via `envFrom`; this pins the other two.
    #[test]
    fn job_mounts_each_file_variable_at_its_mount_path() {
        let rendered = render_template(job_template(), build_context_with_mounted_file());

        assert!(rendered.contains("secretName: config-file-secret"));
        assert!(rendered.contains(r#"mountPath: "/etc/config.json""#));
        // subPath, so the file lands AT the path rather than as a directory containing it.
        assert!(rendered.contains("subPath: content"));
    }

    /// The assertion above cannot tell the escaped form from the raw one — `yaml_encode` renders an
    /// ordinary path identically. This one can: a mount path is customer-controlled, and a quote
    /// followed by a newline at container-spec indentation would otherwise add fields to the pod.
    #[test]
    fn job_mount_path_cannot_inject_manifest_fields() {
        let payload = "/etc/config.json\"\n      hostPID: true\n      dummy: \"x";
        let mut context = build_context_with_mounted_file();
        context.mounted_files[0].mount_path = payload.to_string();

        let rendered = render_template(job_template(), context);
        let job: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered job must parse as YAML");
        let pod_spec = &job["spec"]["template"]["spec"];

        assert!(pod_spec["hostPID"].is_null(), "injected hostPID in:\n{rendered}");
        assert!(pod_spec["dummy"].is_null(), "injected dummy field in:\n{rendered}");
        // the workflow's own mount sits after the engine's, so match on the value
        assert!(
            pod_spec["containers"][0]["volumeMounts"]
                .as_sequence()
                .expect("volumeMounts must be a sequence")
                .iter()
                .any(|mount| mount["mountPath"].as_str() == Some(payload)),
            "the path must land intact inside the mount it was meant for:\n{rendered}"
        );
    }

    #[test]
    fn job_mounts_nothing_extra_when_the_workflow_has_no_file_variable() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(!rendered.contains("subPath: content"));
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

    /// Deliberately not a plausible hard-coded default, see the io-model tests.
    const TEST_CLUSTER_REGION: &str = "ap-southeast-2";
    const TEST_BEDROCK_REGION: &str = "eu-west-1";

    /// Minimal config used by contract and Bedrock rendering tests.
    fn config_for(model_type: AgenticWorkflowModelType, bedrock_auth: Option<BedrockAuth>) -> AgenticWorkflowConfig {
        AgenticWorkflowConfig {
            image_repository: "public.ecr.aws/r3m4q3r9/qovery-ai-runner".to_string(),
            image_tag: "0.0.2".to_string(),
            docker_fragment: String::new(),
            prompt: String::new(),
            model_type,
            cluster_region: TEST_CLUSTER_REGION.to_string(),
            bedrock: bedrock_auth.map(|auth| BedrockRuntime {
                region: TEST_BEDROCK_REGION.to_string(),
                auth,
            }),
            model_api_key: "sk-anthropic".to_string(),
            model_settings: String::new(),
            mcp: String::new(),
            project_repositories: vec![],
            host_allowlist: vec![],
            outputs: vec![],
            cpu_request_in_milli: KubernetesCpuResourceUnit::MilliCpu(500),
            cpu_limit_in_milli: None,
            ram_request_in_mib: KubernetesMemoryResourceUnit::MebiByte(512),
            ram_limit_in_mib: KubernetesMemoryResourceUnit::MebiByte(1024),
            output_variable_validation_pattern: String::new(),
            max_duration_in_sec: 3_600,
            run_payload: None,
            environment_variables: BTreeMap::new(),
            mounted_files: BTreeSet::new(),
        }
    }

    fn decode(value: &str) -> String {
        String::from_utf8(general_purpose::STANDARD.decode(value).expect("value must be base64")).expect("utf8")
    }

    /// Bedrock must omit the Anthropic key to prevent auth fallback.
    #[test]
    fn a_bedrock_workflow_emits_no_anthropic_api_key() {
        let vars = contract_environment_variables(&config_for(
            AgenticWorkflowModelType::Bedrock,
            Some(BedrockAuth::BearerToken("bedrock-token".to_string())),
        ));

        assert!(
            !vars.iter().any(|ev| ev.key == "ANTHROPIC_API_KEY"),
            "got: {:?}",
            vars.iter().map(|ev| &ev.key).collect::<Vec<_>>()
        );
        // And the Claude path is untouched.
        let claude = contract_environment_variables(&config_for(AgenticWorkflowModelType::Claude, None));
        let key = claude
            .iter()
            .find(|ev| ev.key == "ANTHROPIC_API_KEY")
            .expect("a Claude workflow must still get its key");
        assert_eq!(decode(&key.value), "sk-anthropic");
    }

    /// Bedrock bearer credentials are base64-encoded in Secret `data`.
    #[test]
    fn bearer_token_auth_renders_one_credential_variable() {
        let config = config_for(
            AgenticWorkflowModelType::Bedrock,
            Some(BedrockAuth::BearerToken("bedrock-token".to_string())),
        );
        let (context, credentials) = config.bedrock().expect("a Bedrock workflow must carry a runtime");

        assert_eq!(context.region, TEST_BEDROCK_REGION);
        assert_eq!(
            credentials.iter().map(|ev| ev.key.as_str()).collect::<Vec<_>>(),
            vec!["AWS_BEARER_TOKEN_BEDROCK"]
        );
        assert_eq!(decode(&credentials[0].value), "bedrock-token");
        assert!(credentials.iter().all(|ev| ev.is_secret));
    }

    #[test]
    fn debug_output_redacts_workflow_credentials() {
        let config = config_for(
            AgenticWorkflowModelType::Bedrock,
            Some(BedrockAuth::BearerToken("bedrock-secret-token".to_string())),
        );

        let debug = format!("{config:?}");
        assert!(
            !debug.contains("bedrock-secret-token"),
            "debug leaked the bearer token: {debug}"
        );
        assert!(!debug.contains("sk-anthropic"), "debug leaked the model key: {debug}");
        assert!(debug.contains("***"), "debug should show redaction markers: {debug}");

        let mut tera_context = build_agentic_workflow_tera_context();
        tera_context.bedrock_credentials = vec![EnvironmentVariable {
            key: "AWS_BEARER_TOKEN_BEDROCK".to_string(),
            value: general_purpose::STANDARD.encode("bedrock-secret-token"),
            is_secret: true,
        }];
        let tera_debug = format!("{tera_context:?}");
        assert!(
            !tera_debug.contains("bedrock-secret-token"),
            "tera debug leaked the bearer token: {tera_debug}"
        );
        assert!(!tera_debug.contains(&general_purpose::STANDARD.encode("bedrock-secret-token")));
    }

    /// Bedrock routing requires the switch and AWS environment in Job `env`.
    #[test]
    fn bedrock_job_turns_the_claude_cli_onto_bedrock() {
        let rendered = render_template(job_template(), build_bedrock_tera_context());
        let job: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered job must parse as YAML");
        let env = job["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_sequence()
            .expect("the container must declare env vars");

        let use_bedrock = env
            .iter()
            .find(|var| var["name"].as_str() == Some("CLAUDE_CODE_USE_BEDROCK"))
            .unwrap_or_else(|| panic!("a Bedrock job must set CLAUDE_CODE_USE_BEDROCK:\n{rendered}"));
        assert_eq!(use_bedrock["value"].as_str(), Some("1"));
    }

    /// The Claude path stays free of Bedrock's generated variables, not just of the region.
    #[test]
    fn claude_job_is_not_switched_onto_bedrock() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(!rendered.contains("CLAUDE_CODE_USE_BEDROCK"), "got:\n{rendered}");
    }

    /// Bedrock credentials use decoded Secret `data`; contract values stay in `stringData`.
    #[test]
    fn bedrock_credentials_are_rendered_into_the_decoded_secret_block() {
        let rendered = render_template(secret_template(), build_bedrock_tera_context());
        let secret: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered secret must parse as YAML");

        assert_eq!(
            secret["data"]["AWS_BEARER_TOKEN_BEDROCK"].as_str(),
            Some(general_purpose::STANDARD.encode("bedrock-token").as_str())
        );
        assert!(secret["stringData"]["AWS_BEARER_TOKEN_BEDROCK"].is_null());
        assert!(secret["stringData"]["ANTHROPIC_API_KEY"].is_null());
    }

    /// Bedrock uses Secret `data` for its token and omits `ANTHROPIC_API_KEY`.
    fn build_bedrock_tera_context() -> AgenticWorkflowTeraContext {
        let mut context = build_agentic_workflow_tera_context();
        context.service.bedrock = Some(BedrockTeraContext {
            region: TEST_BEDROCK_REGION.to_string(),
        });
        context.environment_variables.retain(|ev| ev.key != "ANTHROPIC_API_KEY");
        context.bedrock_credentials = vec![EnvironmentVariable {
            key: "AWS_BEARER_TOKEN_BEDROCK".to_string(),
            value: general_purpose::STANDARD.encode("bedrock-token"),
            is_secret: true,
        }];
        context
    }

    /// `AWS_REGION` is plaintext Job env; contract decoding does not apply to it.
    #[test]
    fn bedrock_job_sets_the_selected_provider_region_in_plaintext() {
        let rendered = render_template(job_template(), build_bedrock_tera_context());

        let job: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered job must parse as YAML");
        let env = job["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_sequence()
            .expect("the container must declare env vars");
        let aws_region = env
            .iter()
            .find(|var| var["name"].as_str() == Some("AWS_REGION"))
            .unwrap_or_else(|| panic!("a Bedrock job must set AWS_REGION:\n{rendered}"));

        assert_eq!(aws_region["value"].as_str(), Some(TEST_BEDROCK_REGION));
        // The guard that matters: the value is the region itself, never the engine's base64 of it.
        assert_ne!(
            aws_region["value"].as_str(),
            Some(general_purpose::STANDARD.encode(TEST_BEDROCK_REGION).as_str()),
            "AWS_REGION must not be base64-encoded, nothing decodes it for the claude CLI"
        );
    }

    /// Claude jobs omit `AWS_REGION` so ambient AWS configuration is not overridden.
    #[test]
    fn claude_job_sets_no_aws_region_at_all() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        let job: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered job must parse as YAML");
        let env = job["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_sequence()
            .expect("the container must declare env vars");

        assert!(
            !env.iter().any(|var| var["name"].as_str() == Some("AWS_REGION")),
            "a Claude job must declare no AWS_REGION:\n{rendered}"
        );
    }

    /// The runtime region reaches the Job unchanged.
    #[test]
    fn aws_region_is_emitted_verbatim_from_bedrock_runtime_context() {
        let mut context = build_bedrock_tera_context();
        context.service.bedrock = Some(BedrockTeraContext {
            region: "eu-west-3".to_string(),
        });

        let rendered = render_template(job_template(), context);

        assert!(rendered.contains("value: \"eu-west-3\""), "got:\n{rendered}");
    }

    /// Quoted/newline region input must stay within the `AWS_REGION` scalar.
    #[test]
    fn aws_region_cannot_inject_manifest_fields() {
        let payload = "eu-west-3\"\n      hostPID: true\n      dummy: \"x";
        let mut context = build_bedrock_tera_context();
        context.service.bedrock = Some(BedrockTeraContext {
            region: payload.to_string(),
        });

        let rendered = render_template(job_template(), context);
        let job: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered job must parse as YAML");
        let pod_spec = &job["spec"]["template"]["spec"];

        assert!(pod_spec["hostPID"].is_null(), "injected hostPID in:\n{rendered}");
        assert!(pod_spec["dummy"].is_null(), "injected dummy field in:\n{rendered}");
        assert!(
            pod_spec["containers"][0]["env"]
                .as_sequence()
                .expect("env must be a sequence")
                .iter()
                .any(|var| var["name"].as_str() == Some("AWS_REGION") && var["value"].as_str() == Some(payload)),
            "the region must land intact inside the variable it was meant for:\n{rendered}"
        );
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

    /// Contract values use `stringData` so ai-runner performs the single decode.
    #[test]
    fn secret_uses_string_data_so_container_decode_recovers_canonical_value() {
        let rendered = render_template(secret_template(), build_agentic_workflow_tera_context());

        // Transport must be `stringData` (verbatim), never `data` (which k8s would decode).
        assert!(rendered.contains("stringData:"), "secret must use stringData, got:\n{rendered}");
        // The fixture has no user variables, so the only other block a `data:` key could come
        // from is the contract vars — which must never land there.
        assert!(
            !rendered.contains("\ndata:"),
            "contract vars must not be rendered under `data:`:\n{rendered}"
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

    /// Without a build the Job runs the public base image, so rendering a pull secret would be a
    /// dangling reference to a Secret the chart never creates.
    #[test]
    fn renders_no_pull_secret_when_the_workflow_has_no_built_image() {
        let job = render_template(job_template(), build_agentic_workflow_tera_context());
        let secret = render_template(secret_template(), build_agentic_workflow_tera_context());

        assert!(!job.contains("imagePullSecrets"), "got:\n{job}");
        assert!(!secret.contains("dockerconfigjson"), "got:\n{secret}");
    }

    /// The image built from a docker fragment lands in the cluster's private registry, so the Job
    /// needs the pull secret and the chart has to create it, or the pod is stuck in ImagePullBackOff.
    #[test]
    fn renders_pull_secret_when_the_workflow_image_was_built() {
        let job = render_template(job_template(), build_agentic_workflow_tera_context_with_built_image());
        let secret = render_template(secret_template(), build_agentic_workflow_tera_context_with_built_image());

        assert!(job.contains("imagePullSecrets"), "got:\n{job}");
        assert!(job.contains("- name: test-agentic-workflow-registry"), "got:\n{job}");
        assert!(job.contains("image: \"registry.qovery.com/z1234567:mytag\""), "got:\n{job}");

        assert!(secret.contains("type: kubernetes.io/dockerconfigjson"), "got:\n{secret}");
        assert!(secret.contains("name: test-agentic-workflow-registry"), "got:\n{secret}");
        assert!(secret.contains(".dockerconfigjson: eyJhdXRocyI6e319"), "got:\n{secret}");
    }

    /// Both templates gate on `docker_json_config`, so a registry carrying no credentials renders
    /// neither. Gating the Job on `registry` alone would make it reference a Secret that was never
    /// created; gating the Secret on `registry` alone would create one with an empty
    /// `.dockerconfigjson`, which kubelet rejects. Either way the pod never starts.
    #[test]
    fn renders_no_pull_secret_when_the_registry_carries_no_credentials() {
        let mut context = build_agentic_workflow_tera_context_with_built_image();
        context.registry = Some(RegistryTeraContext {
            secret_name: "test-agentic-workflow-registry".to_string(),
            docker_json_config: None,
        });

        let job = render_template(job_template(), context.clone());
        let secret = render_template(secret_template(), context);

        assert!(!job.contains("imagePullSecrets"), "got:\n{job}");
        assert!(!secret.contains("dockerconfigjson"), "got:\n{secret}");
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

    fn webhook_run_payload() -> AgenticWorkflowRunPayload {
        AgenticWorkflowRunPayload {
            body: "{\"issue\":{\"key\":\"QOV-1\"}}".to_string(),
            headers: vec![
                ("x-atlassian-webhook-identifier".to_string(), "abc-123".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ],
        }
    }

    /// A nested headers object would not parse as `HashMap<String, String>` in `load_inputs`.
    #[test]
    fn run_inputs_json_renders_body_and_headers_as_flat_string_values() {
        let rendered = run_inputs_json(&Some(webhook_run_payload()));

        assert_eq!(
            rendered,
            r#"{"WEBHOOK_BODY":"{\"issue\":{\"key\":\"QOV-1\"}}","WEBHOOK_HEADERS":"{\"content-type\":\"application/json\",\"x-atlassian-webhook-identifier\":\"abc-123\"}"}"#
        );
    }

    /// An empty file would break every run without a triggering event, i.e. every manual deploy.
    #[test]
    fn run_inputs_json_renders_an_empty_object_when_there_is_no_payload() {
        assert_eq!(run_inputs_json(&None), "{}");
    }

    /// RFC 9110 §5.3; last-wins would silently drop a forwarded-for hop.
    #[test]
    fn run_inputs_json_comma_joins_repeated_header_names() {
        let payload = AgenticWorkflowRunPayload {
            body: String::new(),
            headers: vec![
                ("x-forwarded-for".to_string(), "203.0.113.1".to_string()),
                ("x-forwarded-for".to_string(), "203.0.113.2".to_string()),
            ],
        };

        let rendered = run_inputs_json(&Some(payload));

        assert!(
            rendered.contains(r#"\"x-forwarded-for\":\"203.0.113.1, 203.0.113.2\""#),
            "got: {rendered}"
        );
    }

    #[test]
    fn renders_prompt_config_map_with_base64_inputs_json() {
        let rendered = render_template(prompt_config_map_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("inputs.json:"));
        assert!(rendered.contains("eyJXRUJIT09LX0JPRFkiOiJ7XCJpc3N1ZVwiOntcImtleVwiOlwiUU9WLTFcIn19IiwiV0VCSE9PS19IRUFERVJTIjoie1wiY29udGVudC10eXBlXCI6XCJhcHBsaWNhdGlvbi9qc29uXCIsXCJ4LWF0bGFzc2lhbi13ZWJob29rLWlkZW50aWZpZXJcIjpcImFiYy0xMjNcIn0ifQ=="));
    }

    #[test]
    fn renders_job_template_with_inputs_file_mount_and_env_var() {
        let rendered = render_template(job_template(), build_agentic_workflow_tera_context());

        assert!(rendered.contains("INPUTS_FILE"));
        assert!(rendered.contains("/qovery-agentic-workflow/inputs.json"));
        assert!(rendered.contains("subPath: inputs.json"));
    }

    /// The engine base64-encodes every Secret value, so a path routed there would reach the
    /// container encoded and `ai-runner` would open a nonsense filename.
    #[test]
    fn inputs_file_path_is_not_base64_encoded_into_the_secret() {
        let job = render_template(job_template(), build_agentic_workflow_tera_context());
        let secret = render_template(secret_template(), build_agentic_workflow_tera_context());

        assert!(job.contains("value: \"/qovery-agentic-workflow/inputs.json\""));
        assert!(!secret.contains("INPUTS_FILE"));
    }

    fn plain(value: &str) -> VariableInfo {
        VariableInfo {
            value: value.to_string(),
            is_secret: false,
        }
    }

    #[test]
    fn user_variables_round_trip_through_the_secrets_data_block() {
        let mut context = build_agentic_workflow_tera_context();
        // A leading space and a newline are exactly what a `stringData` block scalar would mangle;
        // under `data:` the value is base64 and survives.
        context.user_environment_variables = to_user_environment_variables(
            &BTreeMap::from([("MULTILINE".to_string(), plain(" first\nsecond"))]),
            &Uuid::new_v4(),
            RESERVED_ENVIRONMENT_VARIABLE_NAMES,
        );

        let rendered = render_template(secret_template(), context);

        let encoded = rendered
            .lines()
            .find_map(|line| line.trim().strip_prefix(r#""MULTILINE": "#))
            .map(|value| value.trim_matches('"'))
            .expect("MULTILINE should be rendered");
        let decoded = String::from_utf8(
            general_purpose::STANDARD
                .decode(encoded)
                .expect("kubernetes' single decode must succeed"),
        )
        .expect("decoded value must be valid UTF-8");
        assert_eq!(decoded, " first\nsecond");
    }

    #[test]
    fn user_variable_keys_cannot_inject_manifest_fields() {
        // `to_user_environment_variables` rejects a key like this, but the template must not depend
        // on that: it is the last line of defence if the identifier filter ever loosens.
        let mut context = build_agentic_workflow_tera_context();
        context.user_environment_variables = vec![EnvironmentVariable {
            key: "EVIL\ntype: Opaque\ninjected: true".to_string(),
            value: general_purpose::STANDARD.encode("v"),
            is_secret: false,
        }];

        let rendered = render_template(secret_template(), context);
        let secret: serde_yaml::Value = serde_yaml::Deserializer::from_str(&rendered)
            .map(|doc| serde_yaml::Value::deserialize(doc).expect("rendered secret must parse as YAML"))
            .find(|doc| !doc.is_null())
            .expect("a secret must be rendered");

        assert!(secret["injected"].is_null(), "injected root field in:\n{rendered}");
        assert_eq!(
            secret["data"].as_mapping().expect("data mapping").len(),
            1,
            "one variable must produce exactly one key in:\n{rendered}"
        );
    }

    #[test]
    fn user_variables_render_in_a_stable_order() {
        let mut context = build_agentic_workflow_tera_context();
        context.user_environment_variables = to_user_environment_variables(
            &BTreeMap::from([
                ("ZEBRA".to_string(), plain("z")),
                ("ALPHA".to_string(), plain("a")),
                ("MIKE".to_string(), plain("m")),
            ]),
            &Uuid::new_v4(),
            RESERVED_ENVIRONMENT_VARIABLE_NAMES,
        );

        let rendered = render_template(secret_template(), context);

        let keys = rendered
            .lines()
            .filter_map(|line| line.trim().split_once(':'))
            .map(|(key, _)| key.trim_matches('"').to_string())
            .filter(|key| ["ALPHA", "MIKE", "ZEBRA"].contains(&key.as_str()))
            .collect::<Vec<_>>();
        // Sorted, so a redeploy with unchanged variables does not churn the Helm release.
        assert_eq!(keys, vec!["ALPHA", "MIKE", "ZEBRA"]);
    }

    #[test]
    fn claude_user_variables_keep_aws_credentials() {
        let variables = to_user_environment_variables(
            &BTreeMap::from([
                ("ANTHROPIC_API_KEY".to_string(), plain("stolen")),
                ("AWS_ACCESS_KEY_ID".to_string(), plain("user-access")),
                ("AWS_REGION".to_string(), plain("user-region")),
                ("INPUTS_FILE".to_string(), plain("/tmp/evil.json")),
                ("MY_VAR".to_string(), plain("kept")),
            ]),
            &Uuid::new_v4(),
            RESERVED_ENVIRONMENT_VARIABLE_NAMES,
        );

        assert_eq!(
            variables,
            vec![
                EnvironmentVariable {
                    key: "AWS_ACCESS_KEY_ID".to_string(),
                    value: general_purpose::STANDARD.encode("user-access"),
                    is_secret: false,
                },
                EnvironmentVariable {
                    key: "AWS_REGION".to_string(),
                    value: general_purpose::STANDARD.encode("user-region"),
                    is_secret: false,
                },
                EnvironmentVariable {
                    key: "MY_VAR".to_string(),
                    value: general_purpose::STANDARD.encode("kept"),
                    is_secret: false,
                },
            ]
        );
    }

    #[test]
    fn bedrock_user_variables_drop_unsupported_aws_credential_names() {
        let variables = to_user_environment_variables(
            &BTreeMap::from([
                ("AWS_BEARER_TOKEN_BEDROCK".to_string(), plain("shadow")),
                ("AWS_ACCESS_KEY_ID".to_string(), plain("shadow-access")),
                ("AWS_SECRET_ACCESS_KEY".to_string(), plain("shadow-secret")),
                ("AWS_SESSION_TOKEN".to_string(), plain("shadow-session")),
                ("AWS_REGION".to_string(), plain("shadow-region")),
                ("MY_VAR".to_string(), plain("kept")),
            ]),
            &Uuid::new_v4(),
            &[
                RESERVED_ENVIRONMENT_VARIABLE_NAMES,
                BEDROCK_RESERVED_ENVIRONMENT_VARIABLE_NAMES,
            ]
            .concat(),
        );

        assert_eq!(
            variables,
            vec![
                EnvironmentVariable {
                    key: "AWS_REGION".to_string(),
                    value: general_purpose::STANDARD.encode("shadow-region"),
                    is_secret: false,
                },
                EnvironmentVariable {
                    key: "MY_VAR".to_string(),
                    value: general_purpose::STANDARD.encode("kept"),
                    is_secret: false,
                },
            ]
        );
    }

    #[test]
    fn no_user_variables_renders_no_data_block() {
        let mut context = build_agentic_workflow_tera_context();
        context.user_environment_variables = vec![];

        let rendered = render_template(secret_template(), context);

        // An empty `data:` key would serialize as null, which Kubernetes rejects.
        assert!(
            !rendered.contains("\ndata:"),
            "no user variables must render no data block:\n{rendered}"
        );
    }

    /// Stricter than what Kubernetes accepts for a Secret key: `-` and `.` are legal there but
    /// cannot be a shell identifier, so `envFrom` would drop them with nothing in the engine's
    /// logs to say why.
    #[test]
    fn a_user_variable_name_must_be_an_identifier() {
        let variables = to_user_environment_variables(
            &BTreeMap::from([
                ("MY:KEY".to_string(), plain("colon")),
                ("MY: KEY".to_string(), plain("colon and space")),
                ("MY\nKEY".to_string(), plain("newline")),
                ("".to_string(), plain("empty")),
                ("MY-KEY".to_string(), plain("dash")),
                ("MY.KEY".to_string(), plain("dot")),
                ("1FOO".to_string(), plain("leading digit")),
                ("_MY_VAR2".to_string(), plain("kept")),
            ]),
            &Uuid::new_v4(),
            RESERVED_ENVIRONMENT_VARIABLE_NAMES,
        );

        assert_eq!(
            variables,
            vec![EnvironmentVariable {
                key: "_MY_VAR2".to_string(),
                value: general_purpose::STANDARD.encode("kept"),
                is_secret: false,
            }]
        );
    }

    /// The reason the guard above is in the engine and not left to the API server. A key holding a
    /// newline or a `": "` does not just break its own entry: it breaks the document, and the
    /// registry pull secret rendered after `---` in the same template goes with it.
    #[test]
    fn a_malformed_user_variable_name_cannot_break_the_rendered_secret() {
        let mut context = build_agentic_workflow_tera_context_with_built_image();
        context.user_environment_variables = to_user_environment_variables(
            &BTreeMap::from([
                ("MY\nKEY".to_string(), plain("newline")),
                ("MY: KEY".to_string(), plain("colon and space")),
                ("GOOD_KEY".to_string(), plain("kept")),
            ]),
            &Uuid::new_v4(),
            RESERVED_ENVIRONMENT_VARIABLE_NAMES,
        );

        let rendered = render_template(secret_template(), context);

        let documents = serde_yaml::Deserializer::from_str(&rendered)
            .map(|document| {
                serde_yaml::Value::deserialize(document).expect("every rendered document must be valid YAML")
            })
            .collect::<Vec<_>>();

        let data = documents[0]["data"]
            .as_mapping()
            .expect("the user variables must render a data block");
        assert_eq!(data.len(), 1);
        assert_eq!(data["GOOD_KEY"], general_purpose::STANDARD.encode("kept"));
        // The pull secret still made it out of the same file. Found by type rather than position,
        // so a third document appearing in the template does not turn this into a puzzle.
        assert!(
            documents
                .iter()
                .any(|document| document["type"] == "kubernetes.io/dockerconfigjson"),
            "the registry pull secret must survive a malformed user variable name:\n{rendered}"
        );
    }

    #[test]
    fn a_secret_user_variable_keeps_its_flag_so_it_can_be_masked_in_logs() {
        let variables = to_user_environment_variables(
            &BTreeMap::from([(
                "TOKEN".to_string(),
                VariableInfo {
                    value: "shhh".to_string(),
                    is_secret: true,
                },
            )]),
            &Uuid::new_v4(),
            RESERVED_ENVIRONMENT_VARIABLE_NAMES,
        );

        assert_eq!(variables.len(), 1);
        assert!(variables[0].is_secret);
    }
}
