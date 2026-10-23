use crate::environment::report::application::renderer::render_app_deployment_report;
use crate::environment::report::failure_diagnostics::{
    Target, WorkloadKind, report_failure, report_observed_failure, with_failure_diagnostics,
};
use crate::environment::report::logger::EnvLogger;
use crate::environment::report::{DeploymentReporter, MAX_ELAPSED_TIME_WITHOUT_REPORT};
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, ServiceType};
use std::collections::HashSet;

use crate::environment::models::application::ApplicationService;
use crate::environment::models::container::ContainerService;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::runtime::block_on;
use crate::utilities::to_short_id;
use k8s_openapi::api::core::v1::{Event, PersistentVolumeClaim, Pod, Service};
use kube::Api;
use kube::api::ListParams;
use std::sync::Arc;

use crate::environment::report::recap_reporter::{RecapReporterDeploymentState, render_recap_events};
use crate::environment::report::utils::get_kube_events;
use k8s_openapi::api::apps::v1::ReplicaSet;
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
            kube_client: deployment_target.kube.client(),
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
            kube_client: deployment_target.kube.client(),
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

    fn new_state(&mut self) -> Self::DeploymentState {
        RecapReporterDeploymentState {
            report: "".to_string(),
            timestamp: Instant::now(),
            all_warning_events: vec![],
            failure: None,
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
                self.service_type,
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

        let diagnostics = report_observed_failure(
            &Target {
                kube: &self.kube_client,
                namespace: &self.namespace,
                selector: &self.selector,
                kind: WorkloadKind::Deployment(&self.tag),
            },
            &self.logger,
            None,
            &report.pods,
            &mut last_report.failure,
        );

        // Format the deployment information and send to it to user
        let rendered_report = match render_app_deployment_report(self.service_type, &self.tag, &report) {
            Ok(deployment_status_report) => deployment_status_report,
            Err(err) => {
                self.logger
                    .send_progress(format!("Cannot render deployment status report. Please contact us: {err}"));
                return;
            }
        };

        let rendered_report = with_failure_diagnostics(rendered_report, diagnostics);

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
                if let Some(event_type) = &event.type_
                    && event_type == "Warning"
                {
                    return Some(event);
                }
                None
            })
            .for_each(|event| last_report.all_warning_events.push(event));

        *last_report = RecapReporterDeploymentState {
            report: rendered_report,
            timestamp: Instant::now(),
            all_warning_events: last_report.all_warning_events.clone(),
            failure: last_report.failure.take(),
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
                self.logger
                    .send_success(format!("✅ {} of {} succeeded", self.action, self.service_type));
                return self.logger;
            }
            Err(err) => err,
        };
        warn!("Deployment failed: {error:?}");

        // Special case for app, as if helm timeout this is most likely an issue coming from the user
        if error.tag().is_cancel() {
            self.stop_records(StepStatus::Cancel);
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(
                    r#"
                🚫 {} has been cancelled. {} has been rollback to previous version if rollout was on-going
                "#,
                    self.action, self.service_type
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
                    return self.logger;
                }
            };
            for line in recap_report.trim_end().split('\n').map(str::to_string) {
                self.logger.send_recap(line);
            }

            // Say why, last, so it sits right above the error
            report_failure(
                &Target {
                    kube: &self.kube_client,
                    namespace: &self.namespace,
                    selector: &self.selector,
                    kind: WorkloadKind::Deployment(&self.tag),
                },
                &self.logger,
                last_report.failure,
            );

            // Send error
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(r#"
❌ {} of {} failed but we rollbacked it to previous safe/running version !
⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️
⛑ Look at the Deployment Status Reports above and ask the Qovery AI Copilot for a diagnostic to fix it https://www.qovery.com/docs/copilot/overview
⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️️
                "#, self.action, self.service_type).trim().to_string(),
                None,
            ));
        }

        self.logger
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
pub(crate) struct AppDeploymentReport {
    pub id: Uuid,
    pub pods: Vec<Pod>,
    pub services: Vec<Service>,
    pub pvcs: Vec<PersistentVolumeClaim>,
    pub replicasets: Vec<ReplicaSet>,
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
    let replicaset_api: Api<ReplicaSet> = Api::namespaced(kube.clone(), namespace);

    let list_params = ListParams::default().labels(selector).timeout(15);
    let pods = pods_api.list(&list_params);
    let services = svc_api.list(&list_params);
    let pvcs = pvc_api.list(&list_params);
    let replicasets = replicaset_api.list(&list_params);
    let (pods, services, pvcs, replicasets, events) =
        futures::future::try_join5(pods, services, pvcs, replicasets, get_kube_events(kube.clone(), namespace)).await?;

    Ok(AppDeploymentReport {
        id: *service_id,
        pods: pods.items,
        services: services.items,
        pvcs: pvcs.items,
        replicasets: replicasets.items,
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::report::execute_long_deployment;
    use crate::environment::report::failure_diagnostics::tests::*;
    use crate::environment::report::logger::EnvProgressLogger;
    use crate::events::{EngineEvent, EnvironmentStep, EventMessageVerbosity, Stage};
    use crate::metrics_registry::StdMetricsRegistry;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn reporter(logger: EnvLogger, kube_client: kube::Client) -> ApplicationDeploymentReporter<()> {
        ApplicationDeploymentReporter {
            long_id: Uuid::new_v4(),
            service_type: ServiceType::Application,
            tag: "v2".to_string(),
            namespace: "ns".to_string(),
            kube_client,
            selector: "service=svc".to_string(),
            logger,
            metrics_registry: Arc::new(StdMetricsRegistry::default()),
            _tag: Default::default(),
            action: Action::Create,
        }
    }

    #[test]
    fn terminal_ordering_and_original_result_survive_diagnostic_denial() {
        for readable in [true, false] {
            let (kube, _) = mock_client(move |path, _| {
                if !readable {
                    return denied();
                }
                match path {
                    "pods" => list(vec![pod_json(crash())]),
                    "log" => Reply::Bytes(b"Error SECRET\nTOKEN\n".to_vec()),
                    _ => list(vec![]),
                }
            });
            let (logger, mut receiver) = test_logger();
            let original = deployment_error();
            let expected = format!("{original:?}");
            let task = move |_: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Err(Box::new(original.clone())) };
            let returned = execute_long_deployment(reporter(logger, kube), task).unwrap_err();
            assert_eq!(format!("{returned:?}"), expected);
            let events = sent(&mut receiver);
            assert!(matches!(events.last().unwrap(), EngineEvent::Error(error, _) if error.tag() == returned.tag()));
            assert_eq!(events.iter().filter(|e| matches!(e, EngineEvent::Error(..))).count(), 1);
            let lines = block(&events);
            assert!(!lines.is_empty());
            if !readable {
                assert!(lines[0].contains("cause unavailable"));
            }
            if readable {
                assert!(lines[0].contains("CrashLoopBackOff") && lines.last().unwrap().contains("Error xxx"));
            }
        }
    }

    #[test]
    fn success_and_cancel_make_no_diagnostic_requests() {
        for (cancel, observed) in [(false, false), (true, false), (false, true), (true, true)] {
            let (kube, requests) = mock_client(|path, _| match path {
                "pods" => list(vec![pod_json(crash())]),
                "log" => Reply::Bytes(b"Error observed\n".to_vec()),
                _ => list(vec![]),
            });
            let (logger, mut receiver) = test_logger();
            let mut reporter = reporter(logger, kube);
            let mut state = reporter.new_state();
            if observed {
                reporter.deployment_in_progress(&mut state);
                let events = sent(&mut receiver);
                assert!(
                    events
                        .iter()
                        .any(|event| { event.message(EventMessageVerbosity::SafeOnly).contains("┃ 💭") })
                );
                assert!(block(&events).is_empty());
                requests.lock().unwrap().clear();
            }
            let result = if cancel {
                Err(Box::new(EngineError::new_task_cancellation_requested(event_details(
                    EnvironmentStep::DeployedError,
                ))))
            } else {
                Ok(())
            };
            let _ = reporter.deployment_terminated(&result, state);
            assert!(requests.lock().unwrap().is_empty());
            assert!(block(&sent(&mut receiver)).is_empty());
        }
    }

    #[test]
    fn rollback_replays_observed_failure_before_the_original_terminal_error() {
        let gone = Arc::new(AtomicBool::new(false));
        let pod_gone = gone.clone();
        let (kube, requests) = mock_client(move |path, _| match path {
            "pods" if !pod_gone.load(Ordering::SeqCst) => list(vec![pod_json(crash())]),
            "log" => Reply::Bytes(b"Error boom\n".to_vec()),
            _ => list(vec![]),
        });
        let (logger, mut receiver) = test_logger();
        let mut reporter = reporter(logger, kube);
        let mut state = reporter.new_state();
        reporter.deployment_in_progress(&mut state);
        assert!(sent(&mut receiver).iter().any(|event| {
            event
                .message(EventMessageVerbosity::SafeOnly)
                .contains("┃ 💭 Error boom")
        }));

        // The rollback removed the pod: the final recap must retain the earlier observation.
        gone.store(true, Ordering::SeqCst);
        reporter.deployment_in_progress(&mut state);
        assert!(block(&sent(&mut receiver)).is_empty());
        let result = Err(Box::new(deployment_error()));
        let _ = reporter.deployment_terminated(&result, state);
        let events = sent(&mut receiver);
        let replay = block(&events);
        assert_eq!(replay[0], "Failure observed before rollback:");
        assert!(replay[1].contains("CrashLoopBackOff"));
        assert_eq!(replay[2], "💭 Error boom");
        assert_eq!(
            replay
                .iter()
                .filter(|line| line.as_str() == "Failure observed before rollback:")
                .count(),
            1
        );
        assert!(
            matches!(events.last().unwrap(), EngineEvent::Error(error, _) if error.tag() == result.as_ref().unwrap_err().tag())
        );
        assert_eq!(
            events[events.len() - 2].message(EventMessageVerbosity::SafeOnly),
            *replay.last().unwrap()
        );
        assert_eq!(
            requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path.ends_with("/log"))
                .count(),
            1
        );
    }
    #[test]
    fn observed_diagnostics_are_inside_the_status_report_without_separate_recap_events() {
        let (kube, _) = mock_client(|path, _| match path {
            "pods" => list(vec![pod_json(crash())]),
            "log" => Reply::Bytes(b"Error: deployment failed\n".to_vec()),
            _ => list(vec![]),
        });
        let (logger, mut receiver) = test_logger();
        let mut reporter = reporter(logger, kube);
        let mut state = reporter.new_state();
        reporter.deployment_in_progress(&mut state);
        let events = sent(&mut receiver);
        assert!(
            events
                .iter()
                .all(|event| event.get_details().stage() != &Stage::Environment(EnvironmentStep::Recap,))
        );
        let lines: Vec<_> = events
            .iter()
            .map(|event| event.message(EventMessageVerbosity::SafeOnly))
            .collect();
        let heading = lines
            .iter()
            .position(|line| line.contains("Deployment Status Report"))
            .unwrap();
        let cause = lines
            .iter()
            .position(|line| line.contains("┃") && line.contains("Error: deployment failed"))
            .unwrap();
        let closing = lines.iter().position(|line| line.starts_with('┗')).unwrap();
        assert!(heading < cause && cause < closing);
        reporter.deployment_in_progress(&mut state);
        assert!(
            sent(&mut receiver).is_empty(),
            "Unchanged diagnostics must not repeat the status report"
        );
    }

    #[test]
    fn recovered_pods_do_not_show_retained_failure_in_the_current_report() {
        let recovered = Arc::new(AtomicBool::new(false));
        let ready = Arc::clone(&recovered);
        let (kube, _) = mock_client(move |path, _| match path {
            "pods" if ready.load(Ordering::SeqCst) => list(vec![pod_json(json!({
                "phase": "Running", "conditions": [{"type": "Ready", "status": "True"}],
            }))]),
            "pods" => list(vec![pod_json(crash())]),
            "log" => Reply::Bytes(b"Error: recovered failure\n".to_vec()),
            _ => list(vec![]),
        });
        let (logger, mut receiver) = test_logger();
        let mut reporter = reporter(logger, kube);
        let mut state = reporter.new_state();
        reporter.deployment_in_progress(&mut state);
        let events = sent(&mut receiver);
        assert!(events.iter().any(|event| {
            event
                .message(EventMessageVerbosity::SafeOnly)
                .contains("┃ 💭 Error: recovered failure")
        }));
        recovered.store(true, Ordering::SeqCst);
        reporter.deployment_in_progress(&mut state);
        assert!(
            state.failure.is_some(),
            "Keep historical evidence for a possible terminal failure"
        );
        assert!(sent(&mut receiver).iter().all(|event| {
            !event
                .message(EventMessageVerbosity::SafeOnly)
                .contains("recovered failure")
        }));
    }
}
