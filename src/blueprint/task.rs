use crate::blueprint::models::error::BlueprintError;
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
use std::fmt::Display;
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

    /// Clone the blueprint repository and return the path to the blueprint directory.
    fn clone_blueprint_repo(&self, infra_ctx: &InfrastructureContext) -> Result<PathBuf, Box<EngineError>> {
        let event_details = self.get_event_details(BlueprintStep::LoadConfiguration);
        let request = &self.request.target_environment;

        let workspace = infra_ctx.context().workspace_root_dir();
        let clone_dir = Path::new(workspace).join("blueprint").join(&request.execution_id);

        // Clean up if exists from a previous run
        if clone_dir.exists() {
            let _ = fs::remove_dir_all(&clone_dir);
        }

        fs::create_dir_all(&clone_dir).map_err(|e| {
            Box::new(EngineError::new_invalid_engine_payload(
                event_details.clone(),
                &format!("Failed to create clone directory: {}", e),
                None,
            ))
        })?;

        let blueprint_info = self
            .get_blueprint_info()
            .map_err(|e| Box::new(EngineError::new_blueprint_error(event_details.clone(), e)))?;

        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new(
                format!(
                    "Cloning blueprint repository {} at {} ({})",
                    request.git_url, request.tag, blueprint_info,
                ),
                None,
            ),
        ));

        // Clone the repository
        let git_url = url::Url::parse(&request.git_url).map_err(|e| {
            Box::new(EngineError::new_invalid_engine_payload(
                event_details.clone(),
                &format!("Invalid git URL '{}': {}", request.git_url, e),
                None,
            ))
        })?;

        let git_creds = request.git_credentials.clone();
        git::clone_at_commit(&git_url, &request.tag, &clone_dir, &|_username: &str| match &git_creds {
            Some(creds) => vec![(
                CredentialType::USER_PASS_PLAINTEXT,
                Cred::userpass_plaintext(&creds.login, &creds.access_token).unwrap(),
            )],
            None => vec![],
        })
        .map_err(|e| {
            Box::new(EngineError::new_invalid_engine_payload(
                event_details.clone(),
                &format!("Failed to clone blueprint repo: {:?}", e),
                None,
            ))
        })?;

        // Resolve blueprint path (subdirectory or repo root)
        let full_path = clone_dir.join(blueprint_info.path());

        if !full_path.exists() {
            return Err(Box::new(EngineError::new_invalid_engine_payload(
                event_details,
                &format!("Blueprint path '{}' does not exist in the repository", blueprint_info),
                None,
            )));
        }

        Ok(full_path)
    }

    fn stop_total_steps_records(deployment_ret: &Result<(), Box<EngineError>>, record: StepRecordHandle) {
        let step_status = match deployment_ret {
            Ok(()) => StepStatus::Success,
            Err(err) if err.tag().is_cancel() => StepStatus::Cancel,
            Err(_) => StepStatus::Error,
        };
        record.stop(step_status);
    }

    fn get_blueprint_info(&self) -> Result<BlueprintInfo, BlueprintError> {
        BlueprintInfo::from_tag(&self.request.target_environment.tag)
    }
}

#[derive(Debug)]
pub(crate) struct BlueprintInfo {
    pub provider: String,
    pub service_name: String,
    pub service_version: String,
    pub manifest_version: String,
}

impl BlueprintInfo {
    pub fn from_tag(tag: &str) -> Result<Self, BlueprintError> {
        let split: Vec<&str> = tag.split('/').collect();
        if split.len() != 4 {
            return Err(BlueprintError::InvalidTagFormat);
        }

        Ok(BlueprintInfo {
            provider: split[0].into(),
            service_name: split[1].into(),
            service_version: split[2].into(),
            manifest_version: split[3].into(),
        })
    }

    pub fn path(&self) -> String {
        format!("{}/{}/{}", self.provider, self.service_name, self.service_version)
    }
}

impl Display for BlueprintInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "provider={}, service={}, service version={}, manifest version={}",
            self.provider, self.service_name, self.service_version, self.manifest_version
        )
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
            let _ = self.clone_blueprint_repo(&infra_context)?;
            //TODO: Next step: Manifest Parsing

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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::models::error::BlueprintError;

    // -- BlueprintInfo::from_tag --

    #[test]
    fn from_tag_parses_terraform_blueprint() {
        let info = BlueprintInfo::from_tag("aws/postgres/16/1.0.0").unwrap();
        assert_eq!(info.provider, "aws");
        assert_eq!(info.service_name, "postgres");
        assert_eq!(info.service_version, "16");
        assert_eq!(info.manifest_version, "1.0.0");
    }

    #[test]
    fn from_tag_parses_helm_blueprint() {
        let info = BlueprintInfo::from_tag("helm/redis/7/2.1.0").unwrap();
        assert_eq!(info.provider, "helm");
        assert_eq!(info.service_name, "redis");
        assert_eq!(info.service_version, "7");
        assert_eq!(info.manifest_version, "2.1.0");
    }

    #[test]
    fn from_tag_parses_versionless_service() {
        let info = BlueprintInfo::from_tag("aws/s3/1/1.0.0").unwrap();
        assert_eq!(info.provider, "aws");
        assert_eq!(info.service_name, "s3");
        assert_eq!(info.service_version, "1");
        assert_eq!(info.manifest_version, "1.0.0");
    }

    #[test]
    fn from_tag_rejects_too_few_segments() {
        let err = BlueprintInfo::from_tag("aws/s3/1.0.0").unwrap_err();
        assert_eq!(err, BlueprintError::InvalidTagFormat);
    }

    #[test]
    fn from_tag_rejects_too_many_segments() {
        let err = BlueprintInfo::from_tag("aws/s3/1/1.0.0/extra").unwrap_err();
        assert_eq!(err, BlueprintError::InvalidTagFormat);
    }

    #[test]
    fn from_tag_rejects_empty_string() {
        let err = BlueprintInfo::from_tag("").unwrap_err();
        assert_eq!(err, BlueprintError::InvalidTagFormat);
    }

    #[test]
    fn from_tag_rejects_single_segment() {
        let err = BlueprintInfo::from_tag("aws").unwrap_err();
        assert_eq!(err, BlueprintError::InvalidTagFormat);
    }

    // -- BlueprintInfo::path --

    #[test]
    fn path_returns_three_segments() {
        let info = BlueprintInfo::from_tag("aws/postgres/17/1.1.0").unwrap();
        assert_eq!(info.path(), "aws/postgres/17");
    }

    #[test]
    fn path_excludes_manifest_version() {
        let info = BlueprintInfo::from_tag("helm/redis/7/3.0.0").unwrap();
        // path must not contain the manifest version
        assert!(!info.path().contains("3.0.0"));
        assert_eq!(info.path(), "helm/redis/7");
    }

    // -- BlueprintInfo Display --

    #[test]
    fn display_contains_all_fields() {
        let info = BlueprintInfo::from_tag("gcp/cloud-sql/15/1.2.3").unwrap();
        let display = format!("{}", info);
        assert!(display.contains("provider=gcp"));
        assert!(display.contains("service=cloud-sql"));
        assert!(display.contains("service version=15"));
        assert!(display.contains("manifest version=1.2.3"));
    }
}
