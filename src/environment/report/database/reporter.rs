use crate::environment::models::database::DatabaseService;
use crate::environment::report::database::renderer::render_database_deployment_report;
use crate::environment::report::logger::EnvLogger;
use crate::environment::report::recap_reporter::{RecapReporterDeploymentState, render_recap_events};
use crate::environment::report::{DeploymentReporter, MAX_ELAPSED_TIME_WITHOUT_REPORT};
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, DatabaseType};
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::runtime::block_on;
use crate::utilities::to_short_id;
use k8s_openapi::api::core::v1::{Event, PersistentVolumeClaim, Pod, Service};
use kube::Api;
use kube::api::ListParams;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct DatabaseDeploymentReport {
    pub id: Uuid,
    pub is_managed: bool,
    pub type_: DatabaseType,
    pub version: String,
    pub pods: Vec<Pod>,
    pub services: Vec<Service>,
    pub pvcs: Vec<PersistentVolumeClaim>,
    pub events: Vec<Event>,
}

async fn fetch_database_deployment_report(
    kube: &kube::Client,
    database_id: &Uuid,
    is_managed: bool,
    type_: DatabaseType,
    version: String,
    namespace: &str,
) -> Result<DatabaseDeploymentReport, kube::Error> {
    let selector = format!("databaseId={}", to_short_id(database_id));

    // managed database, fetch only svc and events, the rest is managed by the cloud provider
    if is_managed {
        let svc_api: Api<Service> = Api::namespaced(kube.clone(), namespace);
        let event_api: Api<Event> = Api::namespaced(kube.clone(), namespace);

        let list_params = ListParams::default().labels(&selector).timeout(15);
        let services = svc_api.list(&list_params);
        let events_params = ListParams::default().timeout(15);
        let events = event_api.list(&events_params);
        let (services, events) = futures::future::try_join(services, events).await?;

        return Ok(DatabaseDeploymentReport {
            id: *database_id,
            is_managed,
            type_,
            version,
            pods: vec![],
            services: services.items,
            pvcs: vec![],
            events: events.items,
        });
    }

    // container database, fetch pod, svc, pvc
    let pods_api: Api<Pod> = Api::namespaced(kube.clone(), namespace);
    let svc_api: Api<Service> = Api::namespaced(kube.clone(), namespace);
    let pvc_api: Api<PersistentVolumeClaim> = Api::namespaced(kube.clone(), namespace);
    let event_api: Api<Event> = Api::namespaced(kube.clone(), namespace);

    let list_params = ListParams::default().labels(&selector).timeout(15);
    let pods = pods_api.list(&list_params);
    let services = svc_api.list(&list_params);
    let pvcs = pvc_api.list(&list_params);
    let events_params = ListParams::default().timeout(15);
    let events = event_api.list(&events_params);
    let (pods, services, pvcs, events) = futures::future::try_join4(pods, services, pvcs, events).await?;

    Ok(DatabaseDeploymentReport {
        id: *database_id,
        is_managed,
        type_,
        version,
        pods: pods.items,
        services: services.items,
        pvcs: pvcs.items,
        events: events.items,
    })
}

pub struct DatabaseDeploymentReporter {
    long_id: Uuid,
    namespace: String,
    is_managed: bool,
    type_: DatabaseType,
    version: String,
    kube_client: kube::Client,
    logger: EnvLogger,
    metrics_registry: Arc<dyn MetricsRegistry>,
    action: Action,
}

impl DatabaseDeploymentReporter {
    pub fn new(
        db: &impl DatabaseService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> DatabaseDeploymentReporter {
        DatabaseDeploymentReporter {
            long_id: *db.long_id(),
            namespace: deployment_target.environment.namespace().to_string(),
            is_managed: db.is_managed_service(),
            type_: db.db_type(),
            version: db.version(),
            kube_client: deployment_target.kube.client(),
            logger: deployment_target.env_logger(db, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            action,
        }
    }
}

impl DeploymentReporter for DatabaseDeploymentReporter {
    type DeploymentResult = ();
    type DeploymentState = RecapReporterDeploymentState;
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&mut self) -> Self::DeploymentState {
        RecapReporterDeploymentState {
            report: "".to_string(),
            timestamp: Instant::now(),
            all_warning_events: vec![],
        }
    }

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        self.metrics_registry
            .start_record(self.long_id, StepLabel::Service, StepName::Deployment);
        // managed db
        if self.is_managed {
            self.logger.send_progress(format!(
                "🚀 {} of managed database `{}` is starting",
                self.action,
                to_short_id(&self.long_id)
            ));
            return;
        }

        // container db
        if let Ok(deployment_info) = block_on(fetch_database_deployment_report(
            &self.kube_client,
            &self.long_id,
            self.is_managed,
            self.type_,
            self.version.clone(),
            &self.namespace,
        )) {
            self.logger.send_progress(format!(
                "🚀 {} of container database `{}` is starting: You have {} pod(s) running, {} service(s) running, {} network volume(s)",
                self.action,
                to_short_id(&self.long_id),
                deployment_info.pods.len(),
                deployment_info.services.len(),
                deployment_info.pvcs.len()
            ));
        }
    }

    fn deployment_in_progress(&self, last_report: &mut Self::DeploymentState) {
        // Fetch deployment information from kube api
        let report = match block_on(fetch_database_deployment_report(
            &self.kube_client,
            &self.long_id,
            self.is_managed,
            self.type_,
            self.version.clone(),
            &self.namespace,
        )) {
            Ok(deployment_info) => deployment_info,
            Err(err) => {
                self.logger
                    .send_warning(format!("Error while retrieving deployment information: {err}"));
                return;
            }
        };

        // Format the deployment information and send to it to user
        let rendered_report = match render_database_deployment_report(&report) {
            Ok(deployment_status_report) => deployment_status_report,
            Err(err) => {
                self.logger
                    .send_progress(format!("Cannot render deployment status report. Please contact us: {err}"));
                return;
            }
        };

        // Managed database don't make any progress, so display the message from time to time
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
        self,
        result: &Result<Self::DeploymentResult, Box<EngineError>>,
        last_report: Self::DeploymentState,
    ) -> EnvLogger {
        let error = match result {
            Ok(_) => {
                self.stop_records(StepStatus::Success);
                if self.is_managed {
                    self.logger
                        .send_success(format!("✅ {} of managed database succeeded", self.action));
                } else {
                    self.logger
                        .send_success(format!("✅ {} of container database succeeded", self.action));
                }
                return self.logger;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            self.stop_records(StepStatus::Cancel);
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(
                    r#"
                🚫 {} has been cancelled. Database has been rollback to previous version if rollout was on-going
                "#,
                    self.action
                )
                .trim()
                .to_string(),
                None,
            ));
            return self.logger;
        }

        // Send error recap
        let recap_report = match render_recap_events(&last_report.all_warning_events) {
            Ok(report) => report,
            Err(err) => {
                self.logger
                    .send_progress(format!("Cannot render deployment recap report. Please contact us: {err}"));
                return self.logger;
            }
        };
        for line in recap_report.trim_end().split('\n').map(str::to_string) {
            self.logger.send_recap(line);
        }

        // Send error
        self.stop_records(StepStatus::Error);
        self.logger.send_error(EngineError::new_engine_error(
            *error.clone(),
            format!(r#"
❌ {} of Database failed but we rollbacked it to previous safe/running version !
⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️
⛑ Look at the Deployment Status Reports above and use our troubleshooting guide to fix it https://hub.qovery.com/docs/using-qovery/troubleshoot/
⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️
                "#, self.action).trim().to_string(),
            None,
        ));

        self.logger
    }
}

impl DatabaseDeploymentReporter {
    pub(crate) fn stop_records(&self, step_status: StepStatus) {
        self.metrics_registry
            .stop_record(self.long_id, StepName::Deployment, step_status.clone());
        self.metrics_registry
            .stop_record(self.long_id, StepName::Total, step_status);
    }
}
