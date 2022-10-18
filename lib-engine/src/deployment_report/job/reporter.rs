use crate::cloud_provider::service::{Action, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::logger::EnvLogger;
use crate::deployment_report::DeploymentReporter;
use crate::errors::EngineError;

use k8s_openapi::api::core::v1::{Event, PersistentVolumeClaim, Pod, Service};
use kube::api::ListParams;
use kube::Api;

use crate::models::job::JobService;
use std::time::{Duration, Instant};
use uuid::Uuid;

const _MAX_ELASPED_TIME_WITHOUT_REPORT: Duration = Duration::from_secs(60 * 2);

pub struct JobDeploymentReporter {
    _long_id: Uuid,
    _service_type: ServiceType,
    _tag: String,
    _namespace: String,
    _kube_client: kube::Client,
    _selector: String,
    _last_report: (String, Instant),
    _logger: EnvLogger,
}

impl JobDeploymentReporter {
    pub fn new(app: &impl JobService, deployment_target: &DeploymentTarget, action: Action) -> JobDeploymentReporter {
        JobDeploymentReporter {
            _long_id: *app.long_id(),
            _service_type: ServiceType::Application,
            _tag: app.image_full(),
            _namespace: deployment_target.environment.namespace().to_string(),
            _kube_client: deployment_target.kube.clone(),
            _selector: app.selector().unwrap_or_default(),
            _last_report: ("".to_string(), Instant::now()),
            _logger: deployment_target.env_logger(app, action.to_environment_step()),
        }
    }
}

impl DeploymentReporter for JobDeploymentReporter {
    type DeploymentResult = Result<(), EngineError>;

    fn before_deployment_start(&mut self) {
        println!("start");
    }

    fn deployment_in_progress(&mut self) {
        println!("progress");
    }
    fn deployment_terminated(&mut self, _result: &Self::DeploymentResult) {
        println!("terminated");
    }
}

#[derive(Debug)]
pub(super) struct AppDeploymentReport {
    pub _id: Uuid,
    pub _pods: Vec<Pod>,
    pub _services: Vec<Service>,
    pub _pvcs: Vec<PersistentVolumeClaim>,
    pub _events: Vec<Event>,
}

async fn _fetch_app_deployment_report(
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
        _id: *service_id,
        _pods: pods.items,
        _services: services.items,
        _pvcs: pvcs.items,
        _events: events.items,
    })
}
