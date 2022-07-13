use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::application::renderer::render_app_deployment_report;
use crate::deployment_report::logger::{get_loggers, Loggers};
use crate::deployment_report::DeploymentReporter;
use crate::errors::EngineError;
use crate::models::application::ApplicationService;
use crate::runtime::block_on;
use crate::utilities::to_short_id;
use k8s_openapi::api::core::v1::{Event, PersistentVolumeClaim, Pod, Service};
use kube::api::ListParams;
use kube::Api;
use uuid::Uuid;

pub struct ApplicationDeploymentReporter {
    long_id: Uuid,
    commit: String,
    namespace: String,
    kube_client: kube::Client,
    last_report: String,
    send_progress: Box<dyn Fn(String) + Send>,
    send_success: Box<dyn Fn(String) + Send>,
    send_error: Box<dyn Fn(EngineError) + Send>,
}

impl ApplicationDeploymentReporter {
    pub fn new(
        app: &impl ApplicationService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> ApplicationDeploymentReporter {
        let Loggers {
            send_progress,
            send_success,
            send_error,
        } = get_loggers(app, action);

        ApplicationDeploymentReporter {
            long_id: *app.long_id(),
            commit: app.get_build().git_repository.commit_id.clone(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.clone(),
            last_report: "".to_string(),
            send_progress,
            send_success,
            send_error,
        }
    }
}

impl DeploymentReporter for ApplicationDeploymentReporter {
    type DeploymentResult = Result<(), EngineError>;

    fn before_deployment_start(&mut self) {
        if let Ok(deployment_info) =
            block_on(fetch_app_deployment_report(&self.kube_client, &self.long_id, &self.namespace))
        {
            (self.send_progress)(format!(
                "🚀 Deployment of application `{}` at commit {} is starting: You have {} pod(s) running, {} service(s) running, {} network volume(s)",
                to_short_id(&self.long_id),
                self.commit,
                deployment_info.pods.len(),
                deployment_info.services.len(),
                deployment_info.pvcs.len()
            ));
        }
    }

    fn deployment_in_progress(&mut self) {
        // Fetch deployment information from kube api
        let report = match block_on(fetch_app_deployment_report(&self.kube_client, &self.long_id, &self.namespace)) {
            Ok(deployment_info) => deployment_info,
            Err(err) => {
                (self.send_progress)(format!("Error while retrieving deployment information: {}", err));
                return;
            }
        };

        // Format the deployment information and send to it to user
        let rendered_report = match render_app_deployment_report(&self.commit, &report) {
            Ok(deployment_status_report) => deployment_status_report,
            Err(err) => {
                (self.send_progress)(format!("Cannot render deployment status report. Please contact us: {}", err));
                return;
            }
        };

        if rendered_report == self.last_report {
            return;
        }
        self.last_report = rendered_report;

        // Send it to user
        for line in self.last_report.trim_end().split('\n').map(str::to_string) {
            (self.send_progress)(line);
        }
    }
    fn deployment_terminated(&mut self, result: Self::DeploymentResult) {
        let error = match result {
            Ok(_) => {
                (self.send_success)("✅ Deployment of application succeeded".to_string());
                return;
            }
            Err(err) => err,
        };

        (self.send_error)(EngineError::new_engine_error(
            error.clone(),
            "❌ Deployment of application failed ! Look at the report above and/or internal error below to understand why".to_string(),
            None,
        ));
        (self.send_error)(error);
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
    app_id: &Uuid,
    namespace: &str,
) -> Result<AppDeploymentReport, kube::Error> {
    let selector = format!("appId={}", to_short_id(app_id));
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

    Ok(AppDeploymentReport {
        id: *app_id,
        pods: pods.items,
        services: services.items,
        pvcs: pvcs.items,
        events: events.items,
    })
}
