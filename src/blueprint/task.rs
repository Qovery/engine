use crate::blueprint::action::{deploy_helm, deploy_terraform};
use crate::blueprint::models::error::BlueprintError;
use crate::blueprint::models::info::BlueprintInfo;
use crate::blueprint::models::qovery_blueprint_manifest::{BlueprintKind, QoveryBlueprintManifest};
use crate::blueprint::models::spec::ResolvedBlueprintSpec;
use crate::cmd::docker::Docker;
use crate::cmd::git;
use crate::engine_task::Task;
use crate::engine_task::qovery_api::QoveryApi;
use crate::environment::models::abort::{Abort, AbortStatus, AtomicAbortStatus};
use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::{BlueprintStep, EngineEvent, EventDetails, EventMessage, Stage};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::io_models::context::Context;
use crate::io_models::engine_request::{BlueprintEngineRequest, CloudProviderOptions};
use crate::log_file_writer::LogFileWriter;
use crate::logger::Logger;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepRecordHandle, StepStatus};
use crate::{engine_task, hack};
use git2::{Cred, CredentialType};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::{env, fs};
use tokio::sync::broadcast;

pub struct BlueprintTask {
    workspace_root_dir: String,
    lib_root_dir: String,
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
        )
    }

    fn get_event_details(&self, step: BlueprintStep) -> EventDetails {
        EventDetails::clone_changing_stage(self.request.event_details(), Stage::Blueprint(step))
    }

    fn get_secrets(request: &BlueprintEngineRequest) -> Vec<String> {
        let mut secrets = vec![];

        request.target_environment.variables.iter().for_each(|var| {
            if var.is_secret {
                secrets.push(var.value.clone());
            }
        });

        // Cloud provider secrets
        match &request.cloud_provider.options {
            CloudProviderOptions::Aws { secret_access_key, .. } => {
                secrets.push(secret_access_key.to_string());
            }
            CloudProviderOptions::Scaleway {
                scaleway_secret_key, ..
            } => {
                secrets.push(scaleway_secret_key.to_string());
            }
            CloudProviderOptions::Gcp { gcp_credentials } => {
                secrets.push(gcp_credentials.private_key.to_string());
                if let Ok(json_credentials_raw) = gcp_credentials.try_raw() {
                    secrets.push(json_credentials_raw);
                }
            }
            CloudProviderOptions::Azure { .. } => {}
            CloudProviderOptions::OnPremise { .. } => {}
            CloudProviderOptions::AwsVsphere { .. } => {}
        };

        secrets
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

    fn stop_total_steps_records(deployment_ret: &Result<(), Box<EngineError>>, record: StepRecordHandle) {
        let step_status = match deployment_ret {
            Ok(()) => StepStatus::Success,
            Err(err) if err.tag().is_cancel() => StepStatus::Cancel,
            Err(_) => StepStatus::Error,
        };
        record.stop(step_status);
    }
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

        let deployment_ret = (|| -> Result<(), Box<EngineError>> {
            // 2. Clone blueprint repo
            let (blueprint_dir, blueprint_info) = self.clone_blueprint_repo(&infra_context)?;

            // 3. Parse QBM manifest
            let manifest = self.parse_manifest(&blueprint_dir)?;

            // 4. Resolve spec
            let resolved_spec =
                ResolvedBlueprintSpec::resolve(&manifest.spec, &self.request.target_environment.spec_overrides);

            self.logger.log(EngineEvent::Info(
                self.get_event_details(BlueprintStep::LoadConfiguration),
                EventMessage::new(format!("Resolved blueprint spec: {:?}", resolved_spec), None),
            ));

            // 5. Execute
            let event_details = self.get_event_details(BlueprintStep::Deploy);
            let is_dry_run = infra_context.context().is_dry_run_deploy();

            match resolved_spec {
                ResolvedBlueprintSpec::Terraform(tf_spec) => {
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
                        &blueprint_dir,
                        &self.lib_root_dir,
                        &tf_spec,
                        &self.request.target_environment,
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
                }
                ResolvedBlueprintSpec::Helm(helm_spec) => {
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
                        &self.request.target_environment,
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
                }
            }

            Ok(())
        })();

        Self::stop_total_steps_records(&deployment_ret, record);

        match &deployment_ret {
            Ok(()) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(BlueprintStep::Deployed),
                    EventMessage::new("Blueprint deployment succeeded".to_string(), None),
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
            self.request.test_cluster,
            self.request.features.clone(),
            self.request.metadata.clone(),
            self.docker.clone(),
            self.qovery_api.clone(),
            self.request.event_details(),
        )
    }
}
