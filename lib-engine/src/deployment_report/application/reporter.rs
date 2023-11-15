use crate::cloud_provider::service::{Action, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::application::renderer::render_app_deployment_report;
use crate::deployment_report::logger::EnvLogger;
use crate::deployment_report::{DeploymentReporter, MAX_ELAPSED_TIME_WITHOUT_REPORT};
use crate::errors::EngineError;
use std::collections::HashSet;

use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::models::application::ApplicationService;
use crate::models::container::ContainerService;
use crate::runtime::block_on;
use crate::utilities::to_short_id;
use k8s_openapi::api::core::v1::{Event, PersistentVolumeClaim, Pod, Service};
use kube::api::ListParams;
use kube::Api;
use std::sync::Arc;

use crate::deployment_report::recap_reporter::{render_recap_events, RecapReporterDeploymentState};
use std::time::Instant;
use uuid::Uuid;

pub struct ApplicationDeploymentReporter<T> {
    long_id: Uuid,
    service_type: ServiceType,
    tag: String,
    namespace: String,
    kube_client: kube::Client,
    selector: String,
    logger: EnvLogger,
    metrics_registry: Arc<dyn MetricsRegistry>,
    _tag: std::marker::PhantomData<T>,
    action: Action,
}

impl<T> ApplicationDeploymentReporter<T> {
    pub fn new(
        app: &impl ApplicationService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> ApplicationDeploymentReporter<T> {
        ApplicationDeploymentReporter {
            long_id: *app.long_id(),
            service_type: ServiceType::Application,
            tag: app.version(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.clone(),
            selector: app.kube_label_selector(),
            logger: deployment_target.env_logger(app, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            _tag: Default::default(),
            action,
        }
    }

    pub fn new_for_container(
        container: &impl ContainerService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> ApplicationDeploymentReporter<T> {
        ApplicationDeploymentReporter {
            long_id: *container.long_id(),
            service_type: ServiceType::Container,
            tag: container.version(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.clone(),
            selector: container.kube_label_selector(),
            logger: deployment_target.env_logger(container, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            _tag: Default::default(),
            action,
        }
    }
}

impl<T: Send + Sync> DeploymentReporter for ApplicationDeploymentReporter<T> {
    type DeploymentResult = T;
    type DeploymentState = RecapReporterDeploymentState;
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&self) -> Self::DeploymentState {
        RecapReporterDeploymentState {
            report: "".to_string(),
            timestamp: Instant::now(),
            all_warning_events: vec![],
        }
    }

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        if let Ok(deployment_info) = block_on(fetch_app_deployment_report(
            &self.kube_client,
            &self.long_id,
            &self.selector,
            &self.namespace,
        )) {
            self.metrics_registry
                .start_record(self.long_id, StepLabel::Service, StepName::Deployment);
            self.logger.send_progress(format!(
                "🚀 {} of {} `{}` at tag/commit {} is starting: You have {} pod(s) running, {} service(s) running, {} network volume(s)",
                self.action,
                self.service_type.to_string(),
                to_short_id(&self.long_id),
                self.tag,
                deployment_info.pods.len(),
                deployment_info.services.len(),
                deployment_info.pvcs.len()
            ));
        }
    }

    fn deployment_in_progress(&self, last_report: &mut Self::DeploymentState) {
        // Fetch deployment information from kube api
        let report = match block_on(fetch_app_deployment_report(
            &self.kube_client,
            &self.long_id,
            &self.selector,
            &self.namespace,
        )) {
            Ok(deployment_info) => deployment_info,
            Err(err) => {
                self.logger
                    .send_progress(format!("Error while retrieving deployment information: {err}"));
                return;
            }
        };

        // Format the deployment information and send to it to user
        let rendered_report = match render_app_deployment_report(self.service_type, &self.tag, &report) {
            Ok(deployment_status_report) => deployment_status_report,
            Err(err) => {
                self.logger
                    .send_progress(format!("Cannot render deployment status report. Please contact us: {err}"));
                return;
            }
        };

        // don't spam log same report unless it has been too long time elapsed without one
        if rendered_report == last_report.report && last_report.timestamp.elapsed() < MAX_ELAPSED_TIME_WITHOUT_REPORT {
            return;
        }

        // Compute events' involved object ids to keep only interesting events (e.g remove warning from Horizontal Pod Autoscaler)
        let mut event_uuids_to_keep: HashSet<String> = report
            .pods
            .into_iter()
            .filter_map(|it| it.metadata.uid)
            .collect::<HashSet<String>>();
        event_uuids_to_keep.extend(
            report
                .services
                .into_iter()
                .filter_map(|it| it.metadata.uid)
                .collect::<HashSet<String>>(),
        );
        event_uuids_to_keep.extend(
            report
                .pvcs
                .into_iter()
                .filter_map(|it| it.metadata.uid)
                .collect::<HashSet<String>>(),
        );

        report
            .events
            .clone()
            .into_iter()
            .filter_map(|event| {
                if !event_uuids_to_keep.contains(event.involved_object.uid.as_deref().unwrap_or_default()) {
                    return None;
                }
                if let Some(event_type) = &event.type_ {
                    if event_type == "Warning" {
                        return Some(event);
                    }
                }
                None
            })
            .for_each(|event| last_report.all_warning_events.push(event));

        *last_report = RecapReporterDeploymentState {
            report: rendered_report,
            timestamp: Instant::now(),
            all_warning_events: last_report.all_warning_events.clone(),
        };

        // Send it to user
        for line in last_report.report.trim_end().split('\n').map(str::to_string) {
            self.logger.send_progress(line);
        }
    }

    fn deployment_terminated(
        &self,
        result: &Result<Self::DeploymentResult, Box<EngineError>>,
        last_report: &mut Self::DeploymentState,
    ) {
        let error = match result {
            Ok(_) => {
                self.stop_records(StepStatus::Success);
                self.logger
                    .send_success(format!("✅ {} of {} succeeded", self.action, self.service_type.to_string()));
                return;
            }
            Err(err) => err,
        };

        // Special case for app, as if helm timeout this is most likely an issue coming from the user
        if error.tag().is_cancel() {
            self.stop_records(StepStatus::Cancel);
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(
                    r#"
                🚫 {} has been cancelled. {} has been rollback to previous version if rollout was on-going
                "#,
                    self.action,
                    self.service_type.to_string()
                )
                .trim()
                .to_string(),
                None,
            ));
        } else {
            self.stop_records(StepStatus::Error);

            // Send error recap
            let recap_report = match render_recap_events(&last_report.all_warning_events) {
                Ok(report) => report,
                Err(err) => {
                    self.logger
                        .send_progress(format!("Cannot render deployment recap report. Please contact us: {err}"));
                    return;
                }
            };
            for line in recap_report.trim_end().split('\n').map(str::to_string) {
                self.logger.send_recap(line);
            }

            // Send error
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(r#"
❌ {} of {} failed but we rollbacked it to previous safe/running version !
⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️
Look at the Deployment Status Reports above and use our troubleshooting guide to fix it https://hub.qovery.com/docs/using-qovery/troubleshoot/
⛑ Can't solve the issue? Please have a look at our forum https://discuss.qovery.com/
⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️️
                "#, self.action, self.service_type.to_string()).trim().to_string(),
                None,
            ));
        }
    }
}

impl<T: Send + Sync> ApplicationDeploymentReporter<T> {
    fn stop_records(&self, deployment_status: StepStatus) {
        self.metrics_registry
            .stop_record(self.long_id, StepName::Deployment, deployment_status.clone());
        self.metrics_registry
            .stop_record(self.long_id, StepName::Total, deployment_status);
    }
}

#[derive(Debug)]
pub(super) struct AppDeploymentReport {
    pub id: Uuid,
    pub pods: Vec<Pod>,
    pub services: Vec<Service>,
    pub pvcs: Vec<PersistentVolumeClaim>,
    pub events: Vec<Event>,
}

async fn fetch_app_deployment_report(
    kube: &kube::Client,
    service_id: &Uuid,
    selector: &str,
    namespace: &str,
) -> Result<AppDeploymentReport, kube::Error> {
    let pods_api: Api<Pod> = Api::namespaced(kube.clone(), namespace);
    let svc_api: Api<Service> = Api::namespaced(kube.clone(), namespace);
    let pvc_api: Api<PersistentVolumeClaim> = Api::namespaced(kube.clone(), namespace);
    let event_api: Api<Event> = Api::namespaced(kube.clone(), namespace);

    let list_params = ListParams::default().labels(selector).timeout(15);
    let pods = pods_api.list(&list_params);
    let services = svc_api.list(&list_params);
    let pvcs = pvc_api.list(&list_params);
    let events_params = ListParams::default().timeout(15);
    let events = event_api.list(&events_params);
    let (pods, services, pvcs, events) = futures::future::try_join4(pods, services, pvcs, events).await?;

    Ok(AppDeploymentReport {
        id: *service_id,
        pods: pods.items,
        services: services.items,
        pvcs: pvcs.items,
        events: events.items,
    })
}
