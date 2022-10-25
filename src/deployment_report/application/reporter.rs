use crate::cloud_provider::service::{Action, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::application::renderer::render_app_deployment_report;
use crate::deployment_report::logger::EnvLogger;
use crate::deployment_report::DeploymentReporter;
use crate::errors::EngineError;
use crate::errors::Tag::HelmDeployTimeout;
use crate::models::application::ApplicationService;
use crate::models::container::ContainerService;
use crate::runtime::block_on;
use crate::utilities::to_short_id;
use k8s_openapi::api::core::v1::{Event, PersistentVolumeClaim, Pod, Service};
use kube::api::ListParams;
use kube::Api;

use std::time::{Duration, Instant};
use uuid::Uuid;

const MAX_ELASPED_TIME_WITHOUT_REPORT: Duration = Duration::from_secs(60 * 2);

pub struct ApplicationDeploymentReporter<T> {
    long_id: Uuid,
    service_type: ServiceType,
    tag: String,
    namespace: String,
    kube_client: kube::Client,
    selector: String,
    logger: EnvLogger,
    _tag: std::marker::PhantomData<T>,
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
            tag: app.get_build().git_repository.commit_id.clone(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.clone(),
            selector: app.selector().unwrap_or_default(),
            logger: deployment_target.env_logger(app, action.to_environment_step()),
            _tag: Default::default(),
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
            tag: container.image_full(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.clone(),
            selector: container.selector().unwrap_or_default(),
            logger: deployment_target.env_logger(container, action.to_environment_step()),
            _tag: Default::default(),
        }
    }
}

impl<T: Send + Sync> DeploymentReporter for ApplicationDeploymentReporter<T> {
    type DeploymentResult = T;
    type DeploymentState = (String, Instant);
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&self) -> Self::DeploymentState {
        ("".to_string(), Instant::now())
    }

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        if let Ok(deployment_info) = block_on(fetch_app_deployment_report(
            &self.kube_client,
            &self.long_id,
            &self.selector,
            &self.namespace,
        )) {
            self.logger.send_progress(format!(
                "🚀 Deployment of {} `{}` at tag/commit {} is starting: You have {} pod(s) running, {} service(s) running, {} network volume(s)",
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
                    .send_progress(format!("Error while retrieving deployment information: {}", err));
                return;
            }
        };

        // Format the deployment information and send to it to user
        let rendered_report = match render_app_deployment_report(self.service_type, &self.tag, &report) {
            Ok(deployment_status_report) => deployment_status_report,
            Err(err) => {
                self.logger
                    .send_progress(format!("Cannot render deployment status report. Please contact us: {}", err));
                return;
            }
        };

        // don't spam log same report unless it has been too long time elapsed without one
        if rendered_report == last_report.0 && last_report.1.elapsed() < MAX_ELASPED_TIME_WITHOUT_REPORT {
            return;
        }
        *last_report = (rendered_report, Instant::now());

        // Send it to user
        for line in last_report.0.trim_end().split('\n').map(str::to_string) {
            self.logger.send_progress(line);
        }
    }
    fn deployment_terminated(
        &self,
        result: &Result<Self::DeploymentResult, EngineError>,
        _: &mut Self::DeploymentState,
    ) {
        let error = match result {
            Ok(_) => {
                self.logger
                    .send_success(format!("✅ Deployment of {} succeeded", self.service_type.to_string()));
                return;
            }
            Err(err) => err,
        };

        // Special case for app, as if helm timeout this is most likely an issue coming from the user
        if error.tag().is_cancel() {
            self.logger.send_error(EngineError::new_engine_error(
                error.clone(),
                format!(
                    r#"
                🚫 Deployment has been cancelled. {} has been rollback to previous version if rollout was on-going
                "#,
                    self.service_type.to_string()
                )
                .trim()
                .to_string(),
                None,
            ));
        } else if error.tag() == &HelmDeployTimeout {
            self.logger.send_error(EngineError::new_engine_error(
                error.clone(),
                format!(r#"
❌ {} failed to be deployed in the given time frame.
This most likely an issue with its configuration or because the app failed to start correctly.
Look at the report from above to understand why, and check your applications logs.

⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
                "#, self.service_type.to_string()).trim().to_string(),
                None,
            ));
        } else {
            self.logger.send_error(error.clone());
            self.logger.send_error(EngineError::new_engine_error(
                error.clone(),
                format!(r#"
❌ Deployment of {} failed ! Look at the report above and to understand why.
⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
                "#, self.service_type.to_string()).trim().to_string(),
                None,
            ));
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
