mod azure;
mod delete_kube_apps;
mod deploy_helms;
mod deploy_terraform;
mod eks;
mod gen_metrics_charts;
mod gke;
pub(super) mod kubeconfig_helper;
mod kubectl_utils;
mod scaleway;
mod self_managed;
mod utils;

use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureDiffType, InfrastructureStep};
use crate::infrastructure::action::utils::mk_logger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::{KubernetesUpgradeStatus, is_kubernetes_upgrade_required};
use crate::logger::Logger;
use crate::services::kubernetes_api_deprecation_service::KubernetesApiDeprecationServiceGranuality;
use tera::Context as TeraContext;

pub trait InfrastructureAction: Send + Sync {
    /// Will be called only if it is the first time the cluster is created.
    /// Otherwise, it will be skipped and the `create_cluster` method will be called directly.
    fn bootstap_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Ok(())
    }
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>>;
    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>>;

    fn run(&self, infra_ctx: &InfrastructureContext, action: Action) -> Result<(), Box<EngineError>> {
        let step = match action {
            Action::Create => InfrastructureStep::Create,
            Action::Pause => InfrastructureStep::Pause,
            Action::Delete => InfrastructureStep::Delete,
            Action::Restart => InfrastructureStep::RestartedError,
        };
        let logger = mk_logger(infra_ctx.kubernetes(), step);
        let kubernetes = infra_ctx.kubernetes();
        let cloud_provider = infra_ctx.cloud_provider();
        if infra_ctx.context().is_dry_run_deploy() {
            logger.warn("👻 Dry run mode is enabled. No changes will be made to the infrastructure");
        }

        logger.info(format!(
            "{} {} cluster {}",
            action,
            infra_ctx.kubernetes().kind(),
            infra_ctx.kubernetes().name()
        ));

        match action {
            Action::Create => {
                let mut cluster_has_been_upgraded = false;
                if infra_ctx.context().is_first_cluster_deployment() {
                    self.bootstap_cluster(infra_ctx)?;
                } else if let Some(upgrade_status) = self.is_upgrade_required(infra_ctx) {
                    let kube_client = infra_ctx.mk_kube_client()?;
                    let event_details = kubernetes.get_event_details(Infrastructure(InfrastructureStep::Upgrade));

                    logger.info("Check if cluster has no calls to deprecated kubernetes API in next version");
                    match infra_ctx
                        .kubernetes_api_deprecation_service()
                        .is_cluster_fully_compatible_with_kubernetes_version(
                            kubernetes.kubeconfig_local_file_path().as_path(),
                            Some(&upgrade_status.requested_version),
                            &cloud_provider.credentials_environment_variables(),
                            KubernetesApiDeprecationServiceGranuality::WithQoveryMetadata {
                                kube_client: kube_client.as_ref(),
                            },
                        ) {
                        Ok(_) => logger.info("Cluster is compatible with the next version"),
                        Err(e) => {
                            return Err(Box::new(EngineError::new_k8s_deprecated_api_calls_found_error(
                                event_details.clone(),
                                &upgrade_status.requested_version,
                                e,
                            )));
                        }
                    }

                    cluster_has_been_upgraded = true;
                    self.upgrade_cluster(infra_ctx, upgrade_status)?;
                }

                let cluster = self.create_cluster(infra_ctx, cluster_has_been_upgraded);

                if !infra_ctx.context().is_first_cluster_deployment() {
                    let event_details = kubernetes.get_event_details(Infrastructure(InfrastructureStep::Create));
                    let kube_client = infra_ctx.mk_kube_client()?;
                    let target_kubernetes_version = match kubernetes.version().next_version() {
                        Some(v) => v.into(),
                        None => kubernetes.version().clone().into(),
                    };
                    logger.info(format!(
                        "Check if cluster has calls to deprecated kubernetes API for version `{}`",
                        target_kubernetes_version
                    ));
                    match infra_ctx
                        .kubernetes_api_deprecation_service()
                        .is_cluster_fully_compatible_with_kubernetes_version(
                            kubernetes.kubeconfig_local_file_path().as_path(),
                            Some(&target_kubernetes_version),
                            &cloud_provider.credentials_environment_variables(),
                            KubernetesApiDeprecationServiceGranuality::WithQoveryMetadata {
                                kube_client: kube_client.as_ref(),
                            },
                        ) {
                        Ok(_) => logger.info("Cluster has no calls to deprecated kubernetes API calls"),
                        Err(e) => {
                            // Non blocking error, just more FYI for user, to act on it if needed before upgrading
                            let deprecation_error = EngineError::new_k8s_deprecated_api_calls_found_error(
                                event_details.clone(),
                                &target_kubernetes_version,
                                e,
                            );
                            logger.warn(EventMessage::from(deprecation_error));
                        }
                    }
                }

                cluster
            }
            Action::Pause => self.pause_cluster(infra_ctx),
            Action::Delete => self.delete_cluster(infra_ctx),
            Action::Restart => Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
                infra_ctx
                    .kubernetes()
                    .get_event_details(Infrastructure(InfrastructureStep::RestartedError)),
            ))),
        }
    }

    // During upgrade check we may want to exclude some node as not pertinent/managed by us
    // I.e: fargate nodes are managed by karpenter, so we don't want to upgrade them
    fn upgrade_node_selector(&self) -> Option<&str> {
        None
    }

    fn is_upgrade_required(&self, infra_ctx: &InfrastructureContext) -> Option<KubernetesUpgradeStatus> {
        if infra_ctx.context().is_first_cluster_deployment() || infra_ctx.context().is_dry_run_deploy() {
            return None;
        }

        let event_details = infra_ctx
            .kubernetes()
            .get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Upgrade);
        match is_kubernetes_upgrade_required(
            infra_ctx.kubernetes().kubeconfig_local_file_path(),
            infra_ctx.kubernetes().version().clone(),
            infra_ctx.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            &logger,
            self.upgrade_node_selector(),
        ) {
            Ok(v) if v.required_upgrade_on.is_some() => Some(v),
            Ok(_) => None,
            Err(e) => {
                logger.warn(EventMessage::new(
                    "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                ));
                None
            }
        }
    }
}

pub trait ToInfraTeraContext {
    fn to_infra_tera_context(&self, target: &InfrastructureContext) -> Result<TeraContext, Box<EngineError>>;
}

pub trait InfraLogger {
    fn info(&self, message: impl Into<EventMessage>);
    fn warn(&self, message: impl Into<EventMessage>);
    fn error(self, error: EngineError, message: Option<impl Into<EventMessage>>);

    fn diff(&self, from: InfrastructureDiffType, message: String);
}

struct InfraLoggerImpl {
    event_details: EventDetails,
    logger: Box<dyn Logger>,
}

impl InfraLogger for InfraLoggerImpl {
    fn info(&self, message: impl Into<EventMessage>) {
        self.logger
            .log(EngineEvent::Info(self.event_details.clone(), message.into()));
    }

    fn warn(&self, message: impl Into<EventMessage>) {
        self.logger
            .log(EngineEvent::Warning(self.event_details.clone(), message.into()));
    }

    fn error(self, error: EngineError, message: Option<impl Into<EventMessage>>) {
        self.logger.log(EngineEvent::Error(error, message.map(|ev| ev.into())));
    }

    fn diff(&self, from: InfrastructureDiffType, message: String) {
        let ev = EventDetails::clone_changing_stage(
            self.event_details.clone(),
            Infrastructure(InfrastructureStep::InfrastructureDiff(from)),
        );
        self.logger.log(EngineEvent::Info(ev, EventMessage::from(message)));
    }
}
