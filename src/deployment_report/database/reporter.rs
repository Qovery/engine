use crate::cloud_provider::service::{Action, DatabaseType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::database::renderer::render_database_deployment_report;
use crate::deployment_report::logger::{get_loggers, Loggers};
use crate::deployment_report::DeploymentReporter;
use crate::errors::EngineError;
use crate::models::database::DatabaseService;
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
    send_success: Box<dyn Fn(String) + Send>,
    send_error: Box<dyn Fn(EngineError) + Send>,
}

impl DatabaseDeploymentReporter {
    pub fn new(
        db: &impl DatabaseService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> DatabaseDeploymentReporter {
        let Loggers {
            send_progress,
            send_success,
            send_error,
        } = get_loggers(db, action);

        DatabaseDeploymentReporter {
            long_id: *db.long_id(),
            namespace: deployment_target.environment.namespace().to_string(),
            is_managed: db.is_managed_service(),
            type_: db.db_type(),
            version: db.version(),
            last_report: "".to_string(),
            kube_client: deployment_target.kube.clone(),
            send_progress,
            send_success,
            send_error,
        }
    }
}

impl DeploymentReporter for DatabaseDeploymentReporter {
    type DeploymentResult = Result<(), EngineError>;

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
    fn deployment_terminated(&mut self, result: &Self::DeploymentResult) {
        let error = match result {
            Ok(_) => {
                if self.is_managed {
                    (self.send_success)("✅ Deployment of managed database succeeded".to_string());
                } else {
                    (self.send_success)("✅ Deployment of container database succeeded".to_string());
                }
                return;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            (self.send_error)(EngineError::new_engine_error(
                error.clone(),
                r#"
                🚫 Deployment has been cancelled. Database has been rollback to previous version if rollout was on-going
                "#
                .trim()
                .to_string(),
                None,
            ));
            return;
        }

        (self.send_error)(error.clone());
        (self.send_error)(EngineError::new_engine_error(
            error.clone(),
            r#"
❌ Deployment of database failed ! Look at the report above and to understand why.
⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
                "#.trim().to_string(),
            None,
        ));
    }
}
