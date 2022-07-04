use crate::cloud_provider::service::{Action, DatabaseService, DatabaseType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::database::renderer::render_database_deployment_report;
use crate::deployment_report::DeploymentReporter;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::io_models::ProgressLevel::Info;
use crate::io_models::{ListenersHelper, ProgressInfo};
use crate::runtime::block_on;
use crate::utilities::to_short_id;
use k8s_openapi::api::core::v1::{Event, PersistentVolumeClaim, Pod, Service};
use kube::api::ListParams;
use kube::Api;
use uuid::Uuid;

#[derive(Debug)]
pub(super) struct DatabaseDeploymentReport {
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
    last_report: String,
    kube_client: kube::Client,
    send_progress: Box<dyn Fn(String) + Send>,
}

impl DatabaseDeploymentReporter {
    pub fn new(
        db: &impl DatabaseService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> DatabaseDeploymentReporter {
        // For the logger, lol ...
        let log = {
            let event_details = db.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
            let logger = db.logger().clone_dyn();
            let execution_id = db.context().execution_id().to_string();
            let scope = db.progress_scope();
            let listeners = db.listeners().clone();
            let step = match action {
                Action::Create => EnvironmentStep::Deploy,
                Action::Pause => EnvironmentStep::Pause,
                Action::Delete => EnvironmentStep::Delete,
                Action::Nothing => EnvironmentStep::Deploy, // should not happen
            };
            let event_details = EventDetails::clone_changing_stage(event_details, Stage::Environment(step));

            move |msg: String| {
                let listeners_helper = ListenersHelper::new(&listeners);
                let info = ProgressInfo::new(scope.clone(), Info, Some(msg.clone()), execution_id.clone());
                match action {
                    Action::Create => listeners_helper.deployment_in_progress(info),
                    Action::Pause => listeners_helper.pause_in_progress(info),
                    Action::Delete => listeners_helper.delete_in_progress(info),
                    Action::Nothing => listeners_helper.deployment_in_progress(info),
                };
                logger.log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));
            }
        };

        DatabaseDeploymentReporter {
            long_id: *db.long_id(),
            namespace: deployment_target.environment.namespace().to_string(),
            is_managed: db.is_managed_service(),
            type_: db.db_type(),
            version: db.version(),
            last_report: "".to_string(),
            kube_client: deployment_target.kube.clone(),
            send_progress: Box::new(log),
        }
    }
}

impl DeploymentReporter for DatabaseDeploymentReporter {
    fn before_deployment_start(&mut self) {
        // managed db
        if self.is_managed {
            (self.send_progress)(format!(
                "🚀 Deployment of managed database `{}` is starting",
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
            (self.send_progress)(format!(
                "🚀 Deployment of container database `{}` is starting: You have {} pod(s) running, {} service(s) running, {} network volume(s)",
                to_short_id(&self.long_id),
                deployment_info.pods.len(),
                deployment_info.services.len(),
                deployment_info.pvcs.len()
            ));
        }
    }

    fn deployment_in_progress(&mut self) {
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
                (self.send_progress)(format!("Error while retrieving deployment information: {}", err));
                return;
            }
        };

        // Format the deployment information and send to it to user
        let rendered_report = match render_database_deployment_report(&report) {
            Ok(deployment_status_report) => deployment_status_report,
            Err(err) => {
                (self.send_progress)(format!("Cannot render deployment status report. Please contact us: {}", err));
                return;
            }
        };

        // Managed database don't make any progress, so display the message from time to time
        if !self.is_managed && rendered_report == self.last_report {
            return;
        }
        self.last_report = rendered_report;

        // Send it to user
        for line in self.last_report.trim_end().split('\n').map(str::to_string) {
            (self.send_progress)(line);
        }
    }
}
