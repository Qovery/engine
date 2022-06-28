use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::application::renderer::render_app_deployment_report;
use crate::deployment_report::DeploymentReporter;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::io_models::ProgressLevel::Info;
use crate::io_models::{ListenersHelper, ProgressInfo};
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
    send_progress: Box<dyn Fn(String) + Send>,
}

impl ApplicationDeploymentReporter {
    pub fn new(
        app: &impl ApplicationService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> ApplicationDeploymentReporter {
        // For the logger, lol ...
        let log = {
            let event_details = app.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
            let logger = app.logger().clone_dyn();
            let execution_id = app.context().execution_id().to_string();
            let scope = app.progress_scope();
            let listeners = app.listeners().clone();
            let step = match action {
                Action::Create => EnvironmentStep::Deploy,
                Action::Pause => EnvironmentStep::Pause,
                Action::Delete => EnvironmentStep::Delete,
                Action::Nothing => EnvironmentStep::Deploy, // should not happen
            };
            let event_details = EventDetails::clone_changing_stage(event_details, Stage::Environment(step));

            move |msg: String| {
                let listeners_helper = ListenersHelper::new(&listeners);
                listeners_helper.deployment_in_progress(ProgressInfo::new(
                    scope.clone(),
                    Info,
                    Some(msg.clone()),
                    execution_id.clone(),
                ));
                logger.log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));
            }
        };

        ApplicationDeploymentReporter {
            long_id: *app.long_id(),
            commit: app.get_build().git_repository.commit_id.clone(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.clone(),
            send_progress: Box::new(log),
        }
    }
}

impl DeploymentReporter for ApplicationDeploymentReporter {
    fn before_deployment_start(&self) {
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

    fn deployment_in_progress(&self) {
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

        // Send it to user
        for line in rendered_report.trim_end().split('\n').map(str::to_string) {
            (self.send_progress)(line);
        }
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
