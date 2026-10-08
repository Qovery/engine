use crate::blueprint::action::{deploy_helm, deploy_terraform, diff};
use crate::blueprint::models::error::BlueprintError;
use crate::blueprint::models::info::BlueprintInfo;
use crate::blueprint::models::qovery_blueprint_manifest::{BlueprintKind, QoveryBlueprintManifest};
use crate::blueprint::models::spec::ResolvedBlueprintSpec;
use crate::cmd::command::CommandKiller;
use crate::cmd::docker::Docker;
use crate::cmd::git;
use crate::engine_task::Task;
use crate::engine_task::qovery_api::QoveryApi;
use crate::environment::action::deploy_external_secrets::{
    ExternalSecretReadError, read_service_external_secret_values,
};
use crate::environment::models::abort::{Abort, AbortStatus, AtomicAbortStatus};
use crate::environment::models::types::DeployedEngineVersion;
use crate::environment::report::obfuscation_service::{ObfuscationService, StdObfuscationService};
use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::{BlueprintStep, EngineEvent, EventDetails, EventMessage, Stage};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::io_models::Action;
use crate::io_models::aws_apn_id::AwsApnId;
use crate::io_models::blueprint::{BlueprintRequest, BlueprintVariable};
use crate::io_models::context::Context;
use crate::io_models::engine_request::{BlueprintEngineRequest, CloudProviderOptions};
use crate::log_file_writer::LogFileWriter;
use crate::logger::Logger;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepRecordHandle, StepStatus};
use crate::{engine_task, hack};
use git2::{Cred, CredentialType};
use itertools::Itertools;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::{env, fs};
use tokio::sync::broadcast;

pub struct BlueprintTask {
    workspace_root_dir: String,
    lib_root_dir: String,
    aws_apn_id: AwsApnId,
    engine_version: DeployedEngineVersion,

    docker: Arc<Docker>,
    request: BlueprintEngineRequest,
    cancel_requested: Arc<AtomicAbortStatus>,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    qovery_api: Arc<dyn QoveryApi>,
    span: tracing::Span,
    is_terminated: (RwLock<Option<broadcast::Sender<()>>>, broadcast::Receiver<()>),
    log_file_writer: Option<LogFileWriter>,
}

impl BlueprintTask {
    pub fn new(
        request: BlueprintEngineRequest,
        workspace_root_dir: String,
        lib_root_dir: String,
        aws_apn_id: AwsApnId,
        engine_version: DeployedEngineVersion,
        docker: Arc<Docker>,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        qovery_api: Box<dyn QoveryApi>,
        log_file_writer: Option<LogFileWriter>,
    ) -> Self {
        let span = info_span!("blueprint_task", execution_id = request.id);

        let secrets = Self::get_secrets(&request);
        BlueprintTask {
            workspace_root_dir,
            lib_root_dir,
            aws_apn_id,
            engine_version,
            docker,
            request,
            logger: logger.with_secrets(secrets),
            metrics_registry,
            cancel_requested: Arc::new(AtomicAbortStatus::new(AbortStatus::None)),
            qovery_api: Arc::from(qovery_api),
            span,
            is_terminated: {
                let (tx, rx) = broadcast::channel(1);
                (RwLock::new(Some(tx)), rx)
            },
            log_file_writer,
        }
    }

    fn infrastructure_context(&self) -> Result<InfrastructureContext, Box<EngineError>> {
        self.request.to_infrastructure_context(
            &self.info_context(),
            self.request.event_details(),
            self.logger.clone(),
            self.metrics_registry.clone(),
            false,
            // Blueprints never build/push images — skip the container registry (its creds are
            // often absent, e.g. local replay of a captured payload).
            false,
        )
    }

    fn get_event_details(&self, step: BlueprintStep) -> EventDetails {
        EventDetails::clone_changing_stage(self.request.event_details(), Stage::Blueprint(step))
    }

    fn get_secrets(request: &BlueprintEngineRequest) -> Vec<String> {
        Self::mask_list(&request.target_environment.variables, &request.cloud_provider.options)
    }

    /// Values to obfuscate in every log line: user secrets plus provider credentials.
    fn mask_list(variables: &[BlueprintVariable], options: &CloudProviderOptions) -> Vec<String> {
        variables
            .iter()
            .filter(|var| var.is_secret)
            .map(|var| var.value.clone())
            .chain(Self::cloud_provider_secrets(options))
            // Blank entry builds an empty regex, which matches everywhere and masks the whole log
            .filter(|secret| !secret.trim().is_empty())
            .collect()
    }

    /// Every credential the provider options carry: whatever reaches a log line gets masked.
    fn cloud_provider_secrets(options: &CloudProviderOptions) -> Vec<String> {
        match options {
            CloudProviderOptions::Aws {
                secret_access_key,
                session_token,
                vsphere_password,
                ..
            } => std::iter::once(secret_access_key.to_string())
                .chain(session_token.iter().chain(vsphere_password.iter()).cloned())
                .collect(),
            CloudProviderOptions::AwsVsphere {
                secret_access_key,
                session_token,
                vsphere_password,
                ..
            } => std::iter::once(vsphere_password.to_string())
                .chain(secret_access_key.iter().chain(session_token.iter()).cloned())
                .collect(),
            CloudProviderOptions::Scaleway {
                scaleway_secret_key, ..
            } => vec![scaleway_secret_key.to_string()],
            CloudProviderOptions::Gcp { gcp_credentials } => std::iter::once(gcp_credentials.private_key.to_string())
                .chain(gcp_credentials.try_raw().ok())
                .collect(),
            CloudProviderOptions::GcpAccessToken { access_token, .. } => vec![access_token.to_string()],
            CloudProviderOptions::Azure { client_secret, .. } => vec![client_secret.to_string()],
            CloudProviderOptions::OnPremise { .. } => vec![],
        }
    }

    /// The request with its external secret references replaced by the values ESO synced for the deployed
    /// service, or by [`UNSYNCED_SECRET_PLACEHOLDER`] for the ones not synced yet.
    fn resolve_external_secret_references(
        &self,
        target_env: &BlueprintRequest,
        infra_ctx: &InfrastructureContext,
        event_details: &EventDetails,
    ) -> Result<ResolvedExternalSecrets, Box<EngineError>> {
        let references = &target_env.external_secret_references;
        if references.is_empty() {
            return Ok(ResolvedExternalSecrets {
                request: target_env.clone(),
                masks: vec![],
                unsynced: vec![],
            });
        }

        let synced = match target_env.import_id.as_deref() {
            // Adopt-preview: no deployed service, so nothing synced
            None => HashMap::new(),
            Some(service_id) => {
                let to_engine_error = |e: ExternalSecretReadError| {
                    Box::new(EngineError::new_blueprint_error(event_details.clone(), BlueprintError::from(e)))
                };
                // Without a kubeconfig file the client silently falls back to the engine pod's own cluster
                if !infra_ctx.kubernetes().kubeconfig_local_file_path().exists() {
                    return Err(to_engine_error(ExternalSecretReadError(
                        "no kubeconfig for the target cluster".to_string(),
                    )));
                }
                let kube_client = infra_ctx.mk_kube_client()?;
                read_service_external_secret_values(
                    &kube_client.client(),
                    &target_env.env_kube_name,
                    service_id,
                    references,
                )
                .map_err(to_engine_error)?
            }
        };

        let (substitutions, unsynced) = external_secret_substitutions(references, &synced);
        Ok(ResolvedExternalSecrets {
            request: diff::with_external_secret_values(target_env, &substitutions),
            masks: external_secret_mask_list(synced.into_values()),
            unsynced,
        })
    }

    /// Clone the blueprint repository and return the path + parsed tag info.
    fn clone_blueprint_repo(
        &self,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(PathBuf, BlueprintInfo), Box<EngineError>> {
        let event_details = self.get_event_details(BlueprintStep::LoadConfiguration);
        let request = &self.request.target_environment;

        let workspace = infra_ctx.context().workspace_root_dir();
        let clone_dir = Path::new(workspace).join("blueprint").join(&request.execution_id);

        if clone_dir.exists() {
            let _ = fs::remove_dir_all(&clone_dir);
        }

        fs::create_dir_all(&clone_dir).map_err(|e| {
            Box::new(EngineError::new_blueprint_error(
                event_details.clone(),
                BlueprintError::WorkspaceError(e.to_string()),
            ))
        })?;

        let blueprint_info = BlueprintInfo::try_new(&request.tag)
            .map_err(|e| Box::new(EngineError::new_blueprint_error(event_details.clone(), e)))?;

        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new(
                format!(
                    "Cloning blueprint repository {} at {} ({})",
                    request.git_url, request.tag, blueprint_info
                ),
                None,
            ),
        ));

        let git_url = url::Url::parse(&request.git_url).map_err(|e| {
            Box::new(EngineError::new_blueprint_error(
                event_details.clone(),
                BlueprintError::InvalidGitUrl(request.git_url.clone(), e.to_string()),
            ))
        })?;

        let git_creds = request.git_credentials.clone();

        // Fetch only the tagged leaf folder via partial clone + sparse-checkout (git CLI). On any
        // failure (e.g. self-hosted git without uploadpack.allowFilter), fall back to a full
        // libgit2 clone of the whole tree at the tag.
        let cancel = self.cancel_checker();
        let cmd_killer = CommandKiller::from_cancelable(cancel.as_ref());
        let creds = git_creds.as_ref().map(|c| (c.login.as_str(), c.access_token.as_str()));

        if let Err(e) =
            git::sparse_clone_at_tag(&git_url, &request.tag, &blueprint_info.path(), &clone_dir, creds, &cmd_killer)
        {
            self.logger.log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(format!("Sparse blueprint clone failed, falling back to full clone: {e}"), None),
            ));
            git::clone_at_tag(&git_url, &request.tag, &clone_dir, &|_username: &str| match &git_creds {
                Some(creds) => vec![(
                    CredentialType::USER_PASS_PLAINTEXT,
                    Cred::userpass_plaintext(&creds.login, &creds.access_token).unwrap(),
                )],
                None => vec![],
            })
            .map_err(|e| {
                Box::new(EngineError::new_blueprint_error(
                    event_details.clone(),
                    BlueprintError::CloneError(format!("{:?}", e)),
                ))
            })?;
        }

        let full_path = clone_dir.join(blueprint_info.path());
        if !full_path.exists() {
            return Err(Box::new(EngineError::new_blueprint_error(
                event_details,
                BlueprintError::BlueprintPathNotFound(blueprint_info.path()),
            )));
        }

        Ok((full_path, blueprint_info))
    }

    /// Parse the QBM manifest from the blueprint directory.
    fn parse_manifest(&self, blueprint_dir: &Path) -> Result<QoveryBlueprintManifest, Box<EngineError>> {
        let event_details = self.get_event_details(BlueprintStep::LoadConfiguration);
        let qbm_path = blueprint_dir.join("qbm.yml");

        if !qbm_path.exists() {
            return Err(Box::new(EngineError::new_blueprint_error(
                event_details,
                BlueprintError::ManifestNotFound(qbm_path.display().to_string()),
            )));
        }

        let manifest = QoveryBlueprintManifest::parse(&qbm_path).map_err(|e| {
            Box::new(EngineError::new_blueprint_error(
                event_details.clone(),
                BlueprintError::ManifestParseError(e.to_string()),
            ))
        })?;

        if manifest.kind != BlueprintKind::ServiceBlueprint {
            return Err(Box::new(EngineError::new_blueprint_error(
                event_details,
                BlueprintError::UnsupportedBlueprintKind,
            )));
        }

        self.logger.log(EngineEvent::Info(
            event_details,
            EventMessage::new(
                format!(
                    "Parsed QBM manifest — engine: {:?}, credentials: {:?}, timeout: {:?}",
                    manifest.spec.engine, manifest.spec.credentials.default, manifest.spec.timeout,
                ),
                None,
            ),
        ));

        Ok(manifest)
    }

    fn stop_total_steps_records<T>(deployment_ret: &Result<T, Box<EngineError>>, record: StepRecordHandle) {
        let step_status = match deployment_ret {
            Ok(_) => StepStatus::Success,
            Err(err) if err.tag().is_cancel() => StepStatus::Cancel,
            Err(_) => StepStatus::Error,
        };
        record.stop(step_status);
    }
}

/// Outcome of a [`BlueprintTask::run`] dispatch. Drives which terminal step is emitted.
enum BlueprintTaskOutcome {
    /// Action::Create — service was created via the qovery terraform provider.
    Deployed,
    /// Action::Diff — render+plan produced this human-readable diff text. No mutations.
    Diffed(String),
}

impl Task for BlueprintTask {
    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn run(&self) {
        if self.request.is_self_managed() {
            engine_task::enable_log_file_writer(&self.info_context(), &self.log_file_writer);
        }

        let _span = self.span.enter();
        info!("blueprint task {} started", self.id());

        self.logger.log(EngineEvent::Info(
            self.get_event_details(BlueprintStep::Start),
            EventMessage::new("Qovery Engine starts to execute the blueprint deployment".to_string(), None),
        ));

        let guard = scopeguard::guard((), |_| {
            hack::remove_gke_gcloud_auth_plugin_cache();
            self.logger.log(EngineEvent::Info(
                self.get_event_details(BlueprintStep::Terminated),
                EventMessage::new("Qovery Engine has terminated the blueprint deployment".to_string(), None),
            ));
            let Some(is_terminated_tx) = self.is_terminated.0.write().unwrap().take() else {
                return;
            };
            let _ = is_terminated_tx.send(());
        });

        // 1. Create infrastructure context
        let infra_context = match self.infrastructure_context() {
            Ok(infra_ctx) => infra_ctx,
            Err(err) => {
                self.logger.log(EngineEvent::Error(*err, None));
                return;
            }
        };

        let metrics_registry = Arc::new(infra_context.metrics_registry().clone_dyn());
        let record =
            metrics_registry.start_record(self.request.target_environment.long_id, StepLabel::Service, StepName::Total);

        let target_env = self.request.target_environment.clone();

        let deployment_ret = (|| -> Result<BlueprintTaskOutcome, Box<EngineError>> {
            // 2. Clone blueprint repo
            let (blueprint_dir, blueprint_info) = self.clone_blueprint_repo(&infra_context)?;

            // 3. Parse QBM manifest
            let manifest = self.parse_manifest(&blueprint_dir)?;

            // 5. Resolve spec
            let resolved_spec = ResolvedBlueprintSpec::resolve(&manifest, &target_env.spec_overrides).map_err(|e| {
                Box::new(EngineError::new_blueprint_error(
                    self.get_event_details(BlueprintStep::LoadConfiguration),
                    e,
                ))
            })?;

            self.logger.log(EngineEvent::Info(
                self.get_event_details(BlueprintStep::LoadConfiguration),
                EventMessage::new(format!("Resolved blueprint spec: {:?}", resolved_spec), None),
            ));

            // 6. Dispatch on action — DIFF runs terraform plan only, anything else (Create) deploys.
            let is_diff = matches!(self.request.action, Action::Diff);
            let event_details = self.get_event_details(if is_diff {
                BlueprintStep::Diff
            } else {
                BlueprintStep::Deploy
            });
            let is_dry_run = infra_context.context().is_dry_run_deploy();

            match (is_diff, resolved_spec) {
                (false, ResolvedBlueprintSpec::Terraform(tf_spec)) => {
                    self.logger.log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new(
                            format!(
                                "Executing Terraform blueprint (provider={}, flavor={:?})",
                                tf_spec.provider, tf_spec.flavor
                            ),
                            None,
                        ),
                    ));

                    deploy_terraform::execute(
                        &self.lib_root_dir,
                        &tf_spec,
                        &target_env,
                        &blueprint_info,
                        is_dry_run,
                        &event_details,
                        self.logger.as_ref(),
                    )?;

                    self.logger.log(EngineEvent::Info(
                        event_details,
                        EventMessage::new(
                            "Terraform blueprint completed — service created via Qovery provider".to_string(),
                            None,
                        ),
                    ));
                    Ok(BlueprintTaskOutcome::Deployed)
                }
                (false, ResolvedBlueprintSpec::Helm(helm_spec)) => {
                    self.logger.log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new(
                            format!(
                                "Executing Helm blueprint (chart={}/{})",
                                helm_spec.chart.name, helm_spec.chart.version
                            ),
                            None,
                        ),
                    ));

                    deploy_helm::execute(
                        &blueprint_dir,
                        &self.lib_root_dir,
                        &helm_spec,
                        &target_env,
                        &blueprint_info,
                        is_dry_run,
                        &event_details,
                        self.logger.as_ref(),
                    )?;

                    self.logger.log(EngineEvent::Info(
                        event_details,
                        EventMessage::new(
                            "Helm blueprint completed — service created via Qovery provider".to_string(),
                            None,
                        ),
                    ));
                    Ok(BlueprintTaskOutcome::Deployed)
                }
                (true, ResolvedBlueprintSpec::Terraform(tf_spec)) => {
                    self.logger.log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new(
                            format!(
                                "Diffing Terraform blueprint (provider={}, flavor={:?}) against deployed state",
                                tf_spec.provider, tf_spec.flavor
                            ),
                            None,
                        ),
                    ));
                    let cloud_envs = infra_context.cloud_provider().credentials_environment_variables();
                    let kubeconfig_path = infra_context.kubernetes().kubeconfig_local_file_path();
                    let external_secrets =
                        self.resolve_external_secret_references(&target_env, &infra_context, &event_details)?;
                    // Placeholders are masked in the live output too: only the final diff explains them
                    let placeholders = (0..external_secrets.unsynced.len()).map(unsynced_secret_placeholder);
                    let mut diff_secrets = [Self::get_secrets(&self.request), external_secrets.masks.clone()].concat();
                    diff_secrets.extend(placeholders);
                    sort_longest_first(&mut diff_secrets);
                    let diff_logger = self.logger.with_secrets(diff_secrets);
                    if !external_secrets.unsynced.is_empty() {
                        diff_logger.log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new(unsynced_secrets_notice(&external_secrets.unsynced), None),
                        ));
                    }
                    // Diff errors and the Diffed payload are logged with self.logger, which does not know these values
                    let obfuscation = StdObfuscationService::new(external_secrets.masks);
                    // Label before masking: a short secret value could otherwise mangle a placeholder
                    let present = |text: String| {
                        obfuscation.obfuscate_secrets(label_unsynced_secrets(text, &external_secrets.unsynced))
                    };
                    let diff = diff::diff_underlying_terraform(
                        &blueprint_dir,
                        &external_secrets.request,
                        &cloud_envs,
                        &kubeconfig_path,
                        tf_spec.timeout_sec,
                        tf_spec.flavor.clone(),
                        &event_details,
                        diff_logger.as_ref(),
                    )
                    .map_err(|e| {
                        Box::new(EngineError::new_blueprint_error(
                            event_details.clone(),
                            BlueprintError::TerraformExecutionError(present(
                                e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars),
                            )),
                        ))
                    })?;
                    Ok(BlueprintTaskOutcome::Diffed(present(diff)))
                }
                (true, ResolvedBlueprintSpec::Helm(helm_spec)) => {
                    // Helm-typed blueprints diff at the qovery_helm wrapper level (chart version
                    // pin + rendered values). That's the right granularity: catalog only ships
                    // values.yaml + qbm.yml, so a catalog tag bump's changes are fully captured by
                    // the wrapper resource fields.
                    self.logger.log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new(
                            format!(
                                "Diffing Helm blueprint (chart={}/{}) at the qovery_helm wrapper level",
                                helm_spec.chart.name, helm_spec.chart.version
                            ),
                            None,
                        ),
                    ));
                    let diff = deploy_helm::execute_diff(
                        &blueprint_dir,
                        &self.lib_root_dir,
                        &helm_spec,
                        &target_env,
                        &blueprint_info,
                        &event_details,
                        self.logger.as_ref(),
                    )?;
                    Ok(BlueprintTaskOutcome::Diffed(diff))
                }
            }
        })();

        Self::stop_total_steps_records(&deployment_ret, record);

        match &deployment_ret {
            Ok(BlueprintTaskOutcome::Deployed) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(BlueprintStep::Deployed),
                    EventMessage::new("Blueprint deployment succeeded".to_string(), None),
                ));
            }
            Ok(BlueprintTaskOutcome::Diffed(diff)) => {
                // The plan output goes in full_details — q-core's diff consumer reads it from there.
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(BlueprintStep::Diff),
                    EventMessage::new("Blueprint diff produced".to_string(), Some(diff.clone())),
                ));
            }
            Err(err) if err.tag().is_cancel() => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(BlueprintStep::Cancelled),
                    EventMessage::new("Blueprint deployment has been canceled at user request".to_string(), None),
                ));
            }
            Err(err) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(BlueprintStep::DeployedError),
                    EventMessage::new(
                        "Blueprint deployment failed".to_string(),
                        Some(err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }
        }

        // Early drop guard to notify core task is done
        drop(guard);
        engine_task::disable_log_file_writer(&self.log_file_writer);

        // Upload workspace archive (only in cloud mode)
        if env::var("DEPLOY_FROM_FILE_KIND").is_err() {
            match crate::fs::create_workspace_archive(
                infra_context.context().workspace_root_dir(),
                infra_context.context().execution_id(),
            ) {
                Ok(file) => match engine_task::upload_s3_file(self.request.archive.as_ref(), &file) {
                    Ok(_) => {
                        let _ = fs::remove_file(file).map_err(|err| error!("Cannot remove file {}", err));
                    }
                    Err(e) => error!("Error while uploading archive {}", e),
                },
                Err(err) => error!("{}", err),
            };
        };

        info!("blueprint task {} finished", self.id());
    }

    fn cancel(&self, force_requested: bool) -> bool {
        if self.is_terminated() {
            info!("Skipping cancel action as the task is already terminated.");
            return false;
        }

        self.cancel_requested.store(
            match force_requested {
                true => AbortStatus::UserForceRequested,
                false => AbortStatus::Requested,
            },
            Ordering::Relaxed,
        );
        self.logger.log(EngineEvent::Info(
            self.get_event_details(BlueprintStep::Cancel),
            EventMessage::new("Cancel received, blueprint deployment is going to stop.".to_string(), None),
        ));
        true
    }

    fn cancel_checker(&self) -> Box<dyn Abort> {
        let cancel_requested = self.cancel_requested.clone();
        Box::new(move || cancel_requested.load(Ordering::Relaxed))
    }

    fn is_terminated(&self) -> bool {
        self.is_terminated.0.read().map(|tx| tx.is_none()).unwrap_or(true)
    }

    fn await_terminated(&self) -> broadcast::Receiver<()> {
        self.is_terminated.1.resubscribe()
    }

    fn info_context(&self) -> Context {
        Context::new(
            self.request.organization_long_id,
            self.request.kubernetes.long_id,
            self.request.id.to_string(),
            self.workspace_root_dir.to_string(),
            self.lib_root_dir.to_string(),
            self.engine_version.clone(),
            self.request.test_cluster,
            self.request.features.clone(),
            self.request.metadata.clone(),
            self.aws_apn_id.clone(),
            self.docker.clone(),
            self.qovery_api.clone(),
            self.request.event_details(),
        )
    }
}

struct ResolvedExternalSecrets {
    request: BlueprintRequest,
    masks: Vec<String>,
    unsynced: Vec<String>,
}

/// Stands in for the `index`-th external secret ESO has not synced for the service yet, so the preview still shows
/// what kind of change happens. Letters and digits, starts with a letter, 21+ chars: passes the service catalog's
/// name, username and password rules (Redis/Valkey passwords need 16+).
fn unsynced_secret_placeholder(index: usize) -> String {
    format!("qovPlaceholderSecret{}", index + 1)
}

/// Value to plan each reference with: its synced value, else a placeholder. Also returns the unsynced references, in
/// placeholder order.
fn external_secret_substitutions(
    references: &[String],
    synced: &HashMap<String, String>,
) -> (HashMap<String, String>, Vec<String>) {
    let unsynced: Vec<String> = references
        .iter()
        .filter(|reference| !synced.contains_key(*reference))
        .cloned()
        .collect();
    let substitutions = synced
        .iter()
        .map(|(reference, value)| (reference.clone(), value.clone()))
        .chain(
            unsynced
                .iter()
                .enumerate()
                .map(|(index, reference)| (reference.clone(), unsynced_secret_placeholder(index))),
        )
        .collect();
    (substitutions, unsynced)
}

fn unsynced_secrets_notice(unsynced: &[String]) -> String {
    format!(
        "External secret(s) {} not synced for this service yet: planned with a placeholder, the real value is applied at deploy",
        unsynced.join(", ")
    )
}

fn label_unsynced_secrets(text: String, unsynced: &[String]) -> String {
    if unsynced.is_empty() {
        return text;
    }
    // Highest index first: `qovPlaceholderSecret1` is a prefix of `qovPlaceholderSecret10`
    let labelled = unsynced
        .iter()
        .enumerate()
        .rev()
        .fold(text, |text, (index, reference)| {
            text.replace(
                &unsynced_secret_placeholder(index),
                &format!("<external secret {reference}, value known at deploy>"),
            )
        });
    format!("# {}\n\n{}", unsynced_secrets_notice(unsynced), labelled)
}

/// Heredoc lines and bare JSON leaves shorter than this are not masked: `{` or `true` would blank out the plan.
const MIN_MASKED_LEN: usize = 6;
/// A JSON leaf this long is still masked in its quoted form `"leaf"`, which only matches identical string values.
const MIN_QUOTED_LEAF_LEN: usize = 3;

/// External secret values plus the forms terraform prints them in (see [`printed_forms`]), also for each leaf of a
/// JSON value, which terraform's `jsonencode(...)` view prints one by one. Longest first: the obfuscation regex
/// alternation is leftmost-first.
fn external_secret_mask_list(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut masks: Vec<String> = values
        .into_iter()
        // Blank entry builds an empty regex, which matches everywhere and masks the whole plan
        .filter(|value| !value.trim().is_empty())
        .flat_map(|value| {
            let leaves = json_leaves(&value)
                .into_iter()
                .filter(|leaf| leaf.trim().len() >= MIN_QUOTED_LEAF_LEN)
                .flat_map(|leaf| {
                    let quoted = [terraform_quoted(&leaf), diff::hcl_escape(&leaf)].map(|form| format!("\"{form}\""));
                    let bare = if is_maskable(&leaf) {
                        printed_forms(&leaf)
                    } else {
                        vec![]
                    };
                    quoted.into_iter().chain(bare)
                });
            printed_forms(&value).into_iter().chain(leaves).collect::<Vec<_>>()
        })
        .unique()
        .collect();
    sort_longest_first(&mut masks);
    masks
}

/// Raw, terraform-quoted, HCL-escaped (`$$`, `%%`), and each heredoc line.
fn printed_forms(value: &str) -> Vec<String> {
    let lines = value.lines().filter(|line| is_maskable(line)).map(str::to_string);
    [value.to_string(), terraform_quoted(value), diff::hcl_escape(value)]
        .into_iter()
        .chain(lines)
        .collect()
}

fn terraform_quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn is_maskable(text: &str) -> bool {
    text.trim().len() >= MIN_MASKED_LEN
}

/// String and number leaves of a JSON object or array value; empty for anything else.
fn json_leaves(value: &str) -> Vec<String> {
    fn collect(node: &serde_json::Value, leaves: &mut Vec<String>) {
        match node {
            serde_json::Value::String(leaf) => leaves.push(leaf.clone()),
            serde_json::Value::Number(leaf) => leaves.push(leaf.to_string()),
            serde_json::Value::Array(items) => items.iter().for_each(|item| collect(item, leaves)),
            serde_json::Value::Object(fields) => fields.values().for_each(|field| collect(field, leaves)),
            serde_json::Value::Null | serde_json::Value::Bool(_) => {}
        }
    }
    let mut leaves = Vec::new();
    if let Ok(node @ (serde_json::Value::Object(_) | serde_json::Value::Array(_))) = serde_json::from_str(value) {
        collect(&node, &mut leaves);
    }
    leaves
}

fn sort_longest_first(secrets: &mut [String]) {
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
}

#[cfg(test)]
mod tests {
    use super::BlueprintTask;
    use crate::io_models::blueprint::BlueprintVariable;
    use crate::io_models::engine_request::CloudProviderOptions;

    #[test]
    fn external_secret_mask_list_covers_quoted_tfvars_and_heredoc_forms() {
        let masks = super::external_secret_mask_list(["p\"a$s\tx\r\nline-two\n}".to_string(), "short".to_string()]);

        assert_eq!(
            masks,
            vec![
                r#"p\"a$$s\tx\r\nline-two\n}"#.to_string(),
                r#"p\"a$s\tx\r\nline-two\n}"#.to_string(),
                "p\"a$s\tx\r\nline-two\n}".to_string(),
                "line-two".to_string(),
                "p\"a$s\tx".to_string(),
                "short".to_string(),
            ]
        );
    }

    #[test]
    fn external_secret_substitutions_plans_unsynced_references_with_placeholders() {
        let references: Vec<String> = ["RABBIT_PW", "SYNCED", "API_KEY"].map(String::from).to_vec();
        let synced = std::collections::HashMap::from([("SYNCED".to_string(), "real-value".to_string())]);

        let (substitutions, unsynced) = super::external_secret_substitutions(&references, &synced);

        assert_eq!(unsynced, vec!["RABBIT_PW".to_string(), "API_KEY".to_string()]);
        assert_eq!(substitutions["SYNCED"], "real-value");
        assert_eq!(substitutions["RABBIT_PW"], "qovPlaceholderSecret1");
        assert_eq!(substitutions["API_KEY"], "qovPlaceholderSecret2");
    }

    #[test]
    fn label_unsynced_secrets_names_them_and_replaces_each_placeholder() {
        let unsynced: Vec<String> = (1..=10).map(|n| format!("SECRET_{n}")).collect();
        let plan = r#"~ db_name  = "pfdemo" -> "qovPlaceholderSecret1"
~ password = "old" -> "qovPlaceholderSecret10""#;

        let labelled = super::label_unsynced_secrets(plan.to_string(), &unsynced);

        assert!(labelled.starts_with("# External secret(s) SECRET_1, SECRET_2"));
        assert!(labelled.contains(r#""pfdemo" -> "<external secret SECRET_1, value known at deploy>""#));
        assert!(labelled.contains(r#""old" -> "<external secret SECRET_10, value known at deploy>""#));
        assert!(!labelled.contains("qovPlaceholder"));
        assert_eq!(super::label_unsynced_secrets(plan.to_string(), &[]), plan);
    }

    #[test]
    fn external_secret_mask_list_skips_blank_values() {
        assert!(super::external_secret_mask_list(["".to_string(), "  \n".to_string()]).is_empty());
    }

    #[test]
    fn external_secret_mask_list_masks_json_leaves() {
        let masks = super::external_secret_mask_list([
            r#"{"engine":"app","password":"p${w}rd-1","pin":123456789,"key":"-----BEGIN-----\nMIIEabcdef\n-----END-----"}"#
                .to_string(),
        ]);

        for expected in ["p${w}rd-1", "p$${w}rd-1", "123456789", "MIIEabcdef", "-----BEGIN-----"] {
            assert!(masks.contains(&expected.to_string()), "missing {expected}");
        }
        assert!(masks.contains(&"\"app\"".to_string()));
        assert!(!masks.contains(&"app".to_string()));
    }

    #[test]
    fn aws_sts_credentials_are_all_masked() {
        let secrets = BlueprintTask::cloud_provider_secrets(&CloudProviderOptions::Aws {
            access_key_id: "ASIAEXAMPLE".to_string(),
            secret_access_key: "secret-key".to_string(),
            session_token: Some("session-token".to_string()),
            vsphere_user: None,
            vsphere_password: Some("vsphere-password".to_string()),
        });

        assert_eq!(secrets, vec!["secret-key", "session-token", "vsphere-password"]);
    }

    #[test]
    fn aws_vsphere_credentials_are_all_masked() {
        let secrets = BlueprintTask::cloud_provider_secrets(&CloudProviderOptions::AwsVsphere {
            access_key_id: Some("ASIAEXAMPLE".to_string()),
            secret_access_key: Some("secret-key".to_string()),
            session_token: Some("session-token".to_string()),
            vsphere_user: "vsphere-user".to_string(),
            vsphere_password: "vsphere-password".to_string(),
        });

        assert_eq!(secrets, vec!["vsphere-password", "secret-key", "session-token"]);
    }

    #[test]
    fn azure_client_secret_is_masked() {
        let secrets = BlueprintTask::cloud_provider_secrets(&CloudProviderOptions::Azure {
            client_id: "client-id".to_string(),
            client_secret: "client-secret".to_string(),
            tenant_id: "tenant-id".to_string(),
            subscription_id: "subscription-id".to_string(),
        });

        assert_eq!(secrets, vec!["client-secret"]);
    }

    #[test]
    fn blank_credentials_never_reach_the_mask_list() {
        let variables = vec![
            BlueprintVariable {
                name: "db_password".to_string(),
                value: "  ".to_string(),
                is_secret: true,
            },
            BlueprintVariable {
                name: "db_name".to_string(),
                value: "app".to_string(),
                is_secret: false,
            },
        ];

        let secrets = BlueprintTask::mask_list(
            &variables,
            &CloudProviderOptions::Aws {
                access_key_id: "ASIAEXAMPLE".to_string(),
                secret_access_key: "".to_string(),
                session_token: Some("".to_string()),
                vsphere_user: None,
                vsphere_password: None,
            },
        );

        assert!(secrets.is_empty());
    }
}
