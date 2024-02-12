use crate::deployment_report::utils::Strategy::OnlyWarningIfAny;
use itertools::Itertools;
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateTerminated, ContainerStateWaiting, Event, LoadBalancerStatus, PersistentVolumeClaim,
    Pod, PodStatus, Service, ServiceStatus,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use tera::Tera;

#[derive(Debug, Serialize)]
pub enum DeploymentState {
    Starting,
    Ready,
    Terminating,
    Failing,
}

#[derive(Debug, Serialize)]
pub struct ServiceRenderContext {
    pub name: String,
    pub type_: String,
    pub state: DeploymentState,
    pub message: Option<String>,
    pub events: Vec<EventRenderContext>,
}

#[derive(Debug, Serialize)]
pub struct ReplicaSetRenderContext {
    pub name: String,
    pub status: Option<String>,
    pub events: Vec<EventRenderContext>,
}

#[derive(Debug, Serialize, Default)]
pub struct QContainerStateTerminated {
    pub exit_code: i32,
    pub exit_code_msg: Option<&'static str>,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub finished_at: Option<v1::Time>,
}

#[derive(Debug, Serialize, Default)]
pub struct QContainerState {
    pub restart_count: u32,
    pub last_state: QContainerStateTerminated,
}

#[derive(Debug, Serialize)]
pub struct PodsRenderContext {
    pub nb_pods: usize,
    pub pods_running: Vec<PodRenderContext>,
    pub pods_starting: Vec<PodRenderContext>,
    pub pods_failing: Vec<PodRenderContext>,
    pub pods_terminating: Vec<PodRenderContext>,
}

#[derive(Debug, Serialize)]
pub struct PodRenderContext {
    pub name: String,
    pub state: DeploymentState,
    pub message: Option<String>,
    pub container_states: BTreeMap<String, QContainerState>,
    pub events: Vec<EventRenderContext>,
    pub service_version: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobRenderContext {
    pub name: String,
    pub state: DeploymentState,
    pub message: Option<String>,
    pub events: Vec<EventRenderContext>,
}

#[derive(Debug, Serialize)]
pub struct EventRenderContext {
    pub message: String,
    pub type_: String,
}

#[derive(Debug, Serialize)]
pub struct PvcRenderContext {
    pub name: String,
    pub state: DeploymentState,
    pub events: Vec<EventRenderContext>,
}

pub fn get_tera_instance() -> Tera {
    let mut tera = Tera::default();
    tera.register_filter("fmt_event_type", fmt_event_type);
    tera
}

pub fn fmt_event_type(value: &tera::Value, _: &HashMap<String, tera::Value>) -> Result<tera::Value, tera::Error> {
    // https://github.com/kubernetes/api/blob/7e99a1ef2ccdd2589e9a41f5083a95c375ada0a2/core/v1/types.go#L5671
    match value {
        tera::Value::String(type_) => match type_.as_str() {
            "Normal" => Ok(tera::Value::String("ℹ️".to_string())),
            "Warning" => Ok(tera::Value::String("⚠️".to_string())),
            _ => Ok(value.clone()),
        },
        _ => Err(tera::Error::msg("Bad event type, it must be a string".to_string())),
    }
}

pub fn to_event_context(ev: &Event) -> Option<EventRenderContext> {
    match (&ev.type_, &ev.message) {
        (None, _) | (_, None) => None,
        (Some(type_), Some(msg)) => Some(EventRenderContext {
            message: msg.replace('\n', ""),
            type_: type_.to_string(),
        }),
    }
}

pub fn to_services_render_context(services: &[Service], events: &[Event]) -> Vec<ServiceRenderContext> {
    if services.is_empty() {
        return vec![];
    }

    let mut svc_ctx: Vec<ServiceRenderContext> = Vec::with_capacity(services.len());
    for svc in services {
        // extract values
        let spec = if let Some(spec) = &svc.spec { spec } else { continue };
        let svc_type: &str = spec.type_.as_deref().unwrap_or("");
        let svc_name = if let Some(name) = &svc.metadata.name {
            name
        } else {
            continue;
        };
        let svc_uid = if let Some(uid) = &svc.metadata.uid {
            uid
        } else {
            continue;
        };

        // If there is a deletion timestamp it means the resource have been asked to be deleted
        if svc.metadata.deletion_timestamp.is_some() {
            svc_ctx.push(ServiceRenderContext {
                name: svc_name.to_string(),
                type_: svc_type.to_string(),
                state: DeploymentState::Terminating,
                message: None,
                events: get_last_events_for(events.iter(), svc_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
                    .into_iter()
                    .flat_map(to_event_context)
                    .collect(),
            });
            continue;
        }

        // if it is a load balancer we need to check that it has an external ip/name assigned
        if svc_type == "LoadBalancer" {
            match &svc.status {
                Some(ServiceStatus {
                    load_balancer: Some(LoadBalancerStatus { ingress: Some(ingress) }),
                    ..
                }) if !ingress.is_empty() => {
                    svc_ctx.push(ServiceRenderContext {
                        name: svc_name.to_string(),
                        type_: "cloud load balancer".to_string(),
                        state: DeploymentState::Ready,
                        message: Some("It can take several minutes for the load balancer to be publicly reachable after the first deployment. Beware of negative cache TTL of DNS resolver".to_string()),
                        events: vec![],
                    });
                }
                _ => {
                    // no ip for the LB
                    svc_ctx.push(ServiceRenderContext {
                        name: svc_name.to_string(),
                        type_: svc_type.to_string(),
                        state: DeploymentState::Starting,
                        message: Some("waiting to be assigned an Ip".to_string()),
                        events: get_last_events_for(events.iter(), svc_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
                            .into_iter()
                            .flat_map(to_event_context)
                            .collect(),
                    });
                }
            }

            continue;
        }

        // We only display Cloud provider LoadBalancer for now, not interested to display kube service
        // If it is not an LB (i.e: ClusterIP), we just check it has an internal ip assigned
        //if spec.cluster_ips.as_ref().map_or(0, |ips| ips.len()) > 0 {
        //    svc_ctx.push(ServiceRenderContext {
        //        name: svc_name.to_string(),
        //        type_: "kubernetes load balancer".to_string(),
        //        state: DeploymentState::Ready,
        //        message: None,
        //        events: vec![],
        //    });
        //    continue;
        //}

        //// no ip for the LB
        //svc_ctx.push(ServiceRenderContext {
        //    name: svc_name.to_string(),
        //    type_: "kubernetes load balancer".to_string(),
        //    state: DeploymentState::Starting,
        //    message: Some("waiting to be assigned an Ip".to_string()),
        //    events: get_last_events_for(events.iter(), svc_uid, DEFAULT_MAX_EVENTS)
        //        .flat_map(to_event_context)
        //        .collect(),
        //});

        continue;
    }

    svc_ctx
}

pub fn to_job_render_context(job: &Job, events: &[Event]) -> JobRenderContext {
    let job_name = job.metadata.name.as_deref().unwrap_or("");
    let job_uid = job.metadata.uid.as_deref().unwrap_or("");
    let state = job
        .status
        .as_ref()
        .and_then(|status| status.failed.as_ref())
        .map_or(DeploymentState::Ready, |_| DeploymentState::Failing);
    let message = job
        .status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .and_then(|conditions| conditions.first())
        .map(|condition| {
            format!(
                "{}: {}",
                condition.reason.as_deref().unwrap_or(""),
                condition.message.as_deref().unwrap_or("")
            )
        });

    return JobRenderContext {
        name: job_name.to_string(),
        state,
        message,
        events: get_last_events_for(events.iter(), job_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
            .into_iter()
            .flat_map(to_event_context)
            .collect(),
    };
}

pub fn to_pods_render_context_by_state(
    pods: &[Pod],
    events: &[Event],
) -> (
    Vec<PodRenderContext>,
    Vec<PodRenderContext>,
    Vec<PodRenderContext>,
    Vec<PodRenderContext>,
) {
    if pods.is_empty() {
        return (vec![], vec![], vec![], vec![]);
    }

    let mut pods_failing: Vec<PodRenderContext> = Vec::with_capacity(pods.len());
    let mut pods_starting: Vec<PodRenderContext> = Vec::with_capacity(pods.len());
    let mut pods_terminating: Vec<PodRenderContext> = Vec::with_capacity(pods.len());
    let mut pods_running: Vec<PodRenderContext> = Vec::with_capacity(pods.len());

    for pod in pods {
        let pod_name = pod.metadata.name.as_deref().unwrap_or("");
        let pod_uid = pod.metadata.uid.as_deref().unwrap_or("");

        if pod.metadata.deletion_timestamp.is_some() {
            pods_terminating.push(PodRenderContext {
                name: pod_name.to_string(),
                state: DeploymentState::Terminating,
                message: None,
                container_states: pod.container_states(),
                service_version: pod.service_version(),
                events: vec![],
            });
            continue;
        }

        if let Some(error_reason) = pod.is_failing() {
            pods_failing.push(PodRenderContext {
                name: pod_name.to_string(),
                state: DeploymentState::Failing,
                message: Some(error_reason.to_string()),
                container_states: pod.container_states(),
                service_version: pod.service_version(),
                events: get_last_events_for(events.iter(), pod_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
                    .into_iter()
                    .flat_map(to_event_context)
                    .collect(),
            });
            continue;
        }

        if pod.is_starting() {
            pods_starting.push(PodRenderContext {
                name: pod_name.to_string(),
                state: DeploymentState::Starting,
                message: None,
                container_states: pod.container_states(),
                service_version: pod.service_version(),
                events: get_last_events_for(events.iter(), pod_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
                    .into_iter()
                    .flat_map(to_event_context)
                    .collect(),
            });
            continue;
        }

        pods_running.push(PodRenderContext {
            name: pod_name.to_string(),
            state: DeploymentState::Starting,
            message: None,
            container_states: pod.container_states(),
            service_version: pod.service_version(),
            events: get_last_events_for(events.iter(), pod_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
                .into_iter()
                .flat_map(to_event_context)
                .collect(),
        });
    }

    (pods_starting, pods_terminating, pods_failing, pods_running)
}

pub fn to_replicasets_render_context(replicasets: &[ReplicaSet], events: &[Event]) -> Vec<ReplicaSetRenderContext> {
    if replicasets.is_empty() {
        return vec![];
    }

    let mut replicasets_ctx: Vec<ReplicaSetRenderContext> = Vec::with_capacity(replicasets.len());
    for replicaset in replicasets {
        // extract values
        let replicaset_name = if let Some(name) = &replicaset.metadata.name {
            name
        } else {
            continue;
        };
        let replicaset_uid = if let Some(uid) = &replicaset.metadata.uid {
            uid
        } else {
            continue;
        };

        let events: Vec<_> = get_last_events_for(events.iter(), replicaset_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
            .into_iter()
            .flat_map(to_event_context)
            .collect();

        if !events.is_empty() {
            let replicaset_status = replicaset
                .status
                .as_ref()
                .and_then(|status| status.conditions.as_ref())
                .and_then(|conditions| conditions.first())
                .map(|condition| condition.reason.as_deref().unwrap_or("").to_string());

            replicasets_ctx.push(ReplicaSetRenderContext {
                name: replicaset_name.to_string(),
                status: replicaset_status,
                events,
            });
        }
        continue;
    }

    replicasets_ctx
}

pub fn to_pods_render_context_by_version(
    pods: &[Pod],
    events: &[Event],
    service_version: &str,
) -> (PodsRenderContext, PodsRenderContext) {
    let (pods_starting, pods_terminating, pods_failing, pods_running) = to_pods_render_context_by_state(pods, events);
    let (current_starting, starting_old): (Vec<_>, Vec<_>) = pods_starting
        .into_iter()
        .partition(|p| p.service_version.as_deref() == Some(service_version));
    let (current_terminating, terminating_old): (Vec<_>, Vec<_>) = pods_terminating
        .into_iter()
        .partition(|p| p.service_version.as_deref() == Some(service_version));
    let (current_failing, failing_old): (Vec<_>, Vec<_>) = pods_failing
        .into_iter()
        .partition(|p| p.service_version.as_deref() == Some(service_version));
    let (current_running, running_old): (Vec<_>, Vec<_>) = pods_running
        .into_iter()
        .partition(|p| p.service_version.as_deref() == Some(service_version));

    (
        PodsRenderContext {
            nb_pods: current_running.len() + current_starting.len() + current_failing.len() + current_terminating.len(),
            pods_running: current_running,
            pods_starting: current_starting,
            pods_failing: current_failing,
            pods_terminating: current_terminating,
        },
        PodsRenderContext {
            nb_pods: running_old.len() + starting_old.len() + failing_old.len() + terminating_old.len(),
            pods_running: running_old,
            pods_starting: starting_old,
            pods_failing: failing_old,
            pods_terminating: terminating_old,
        },
    )
}

pub fn to_pvc_render_context(pvcs: &[PersistentVolumeClaim], events: &[Event]) -> Vec<PvcRenderContext> {
    if pvcs.is_empty() {
        return vec![];
    }

    let mut pvcs_context = Vec::with_capacity(pvcs.len());
    for pvc in pvcs {
        let is_terminating = pvc.metadata.deletion_timestamp.is_some();
        // https://github.com/kubernetes/api/blob/7e99a1ef2ccdd2589e9a41f5083a95c375ada0a2/core/v1/types.go#L647
        let is_starting = matches!(pvc.status.as_ref().and_then(|x| x.phase.as_deref()), Some("Pending"));
        let is_failing = matches!(pvc.status.as_ref().and_then(|x| x.phase.as_deref()), Some("Lost"));
        let is_ready = matches!(pvc.status.as_ref().and_then(|x| x.phase.as_deref()), Some("Bound"));
        let pvc_name = pvc.metadata.name.as_deref().unwrap_or("");
        let pvc_uid = pvc.metadata.uid.as_deref().unwrap_or("");

        if is_terminating {
            pvcs_context.push(PvcRenderContext {
                name: pvc_name.to_string(),
                state: DeploymentState::Terminating,
                events: vec![],
            });
            continue;
        }

        if is_failing {
            pvcs_context.push(PvcRenderContext {
                name: pvc_name.to_string(),
                state: DeploymentState::Failing,
                events: get_last_events_for(events.iter(), pvc_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
                    .into_iter()
                    .flat_map(to_event_context)
                    .collect(),
            });
            continue;
        }

        if is_starting {
            pvcs_context.push(PvcRenderContext {
                name: pvc_name.to_string(),
                state: DeploymentState::Starting,
                events: get_last_events_for(events.iter(), pvc_uid, DEFAULT_MAX_EVENTS, OnlyWarningIfAny)
                    .into_iter()
                    .flat_map(to_event_context)
                    .collect(),
            });
            continue;
        }

        if !is_ready {
            error!(
                "PVC should be in ready state but status phase in not 'Bound' but '{:?}'",
                pvc.status
            )
        }

        pvcs_context.push(PvcRenderContext {
            name: pvc_name.to_string(),
            state: DeploymentState::Ready,
            events: vec![],
        });
        continue;
    }

    pvcs_context
}

pub fn exit_code_to_msg(exit_code: i32) -> Option<&'static str> {
    match exit_code {
        0 =>   Some("the container exited successfully"),
        1 =>   Some("the container exited in an user/code error"),
        125 => Some("the docker run command did not execute successfully. Check your entrypoint/command and arguments"),
        126 => Some("a command specified in the image specification could not be invoked. Does the binary exist?"),
        127 => Some("file or directory specified in the image specification was not found"),
        128 => Some("the exit was triggered with an invalid exit code (valid codes are integers between 0-255)"),
        134 => Some("the container aborted itself using the abort() function (SIGABRT)"),
        135 => Some("the container was killed due to an invalid memory access (SIGBUS)"),
        137 => Some("the container was immediately terminated by the operating system via SIGKILL signal"),
        139 => Some("the container attempted to access memory that was not assigned to it and was terminated (SIGSEGV)"),
        143 => Some("the container received warning that it was about to be terminated, then terminated (SIGTERM)"),
        255 => Some("the container exited, returning an exit code outside the acceptable range, meaning the cause of the error is not known"),
        _ => None,
    }
}

pub trait QPodExt {
    fn restart_count(&self) -> u32;
    fn container_states(&self) -> BTreeMap<String, QContainerState>;
    fn is_starting(&self) -> bool;
    fn is_failing(&self) -> Option<&str>;
    fn service_version(&self) -> Option<String>;
}

impl QPodExt for Pod {
    fn restart_count(&self) -> u32 {
        match &self.status {
            None => 0,
            Some(status) => status
                .container_statuses
                .iter()
                .flatten()
                .fold(0, |acc, status| acc + status.restart_count as u32),
        }
    }

    fn container_states(&self) -> BTreeMap<String, QContainerState> {
        match &self.status {
            None => BTreeMap::new(),
            Some(status) => status
                .container_statuses
                .iter()
                .flatten()
                .filter_map(|status| {
                    status.last_state.as_ref().map(|state| {
                        (
                            status.name.clone(),
                            QContainerState {
                                restart_count: status.restart_count as u32,
                                last_state: state
                                    .terminated
                                    .as_ref()
                                    .map(|state| QContainerStateTerminated {
                                        exit_code: state.exit_code,
                                        exit_code_msg: exit_code_to_msg(state.exit_code),
                                        reason: state.reason.clone(),
                                        message: state.message.clone(),
                                        finished_at: state.finished_at.clone(),
                                    })
                                    .unwrap_or_default(),
                            },
                        )
                    })
                })
                .collect(),
        }
    }

    fn is_starting(&self) -> bool {
        // If the pod is in pending phase, it means it starts
        if let Some("Pending") = self.status.as_ref().and_then(|x| x.phase.as_deref()) {
            return true;
        }

        let conditions = match &self.status {
            None => return true,
            Some(status) => status.conditions.as_deref().unwrap_or(&[]),
        };

        // If there is a condition not ready, it means the pod is still starting
        for condition in conditions {
            if condition.status == "False" {
                return true;
            }
        }

        false
    }

    fn is_failing<'a>(&'a self) -> Option<&'a str> {
        // https://stackoverflow.com/questions/57821723/list-of-all-reasons-for-container-states-in-kubernetes
        let is_error_reason = |reason: &str| {
            matches!(
                reason,
                "OOMKilled"
                    | "Error"
                    | "CrashLoopBackOff"
                    | "ErrImagePull"
                    | "ImagePullBackOff"
                    | "CreateContainerConfigError"
                    | "InvalidImageName"
                    | "CreateContainerError"
                    | "ContainerCannotRun"
                    | "DeadlineExceeded"
            )
        };

        let to_error_message = |reason: &'a str| -> &'a str {
            match reason {
                "OOMKilled" => "OOM killed, pod have been killed due to lack of/using too much memory resources. Investigate the leak or increase memory resources.",
                "CrashLoopBackOff" => "Crash loop, pod is restarting too frequently. It might be due to either the crash of your application at startup (check the Live logs) or a wrong configuration of Liveness/Readiness probes (check the application settings)",
                "ErrImagePull" => "Cannot pull the image for your container",
                "ImagePullBackOff" => "Cannot pull the image for your container",
                "Error" => "An undefined error occurred. Look into your applications logs and message below",
                _ => reason,
            }
        };

        // We need to loop over all status of each container in the pod in order to know
        // if there is something fishy or not, not really friendly...
        match self.status.as_ref() {
            Some(PodStatus {
                container_statuses: Some(ref statuses),
                ..
            }) => {
                for status in statuses {
                    match &status.state {
                        Some(ContainerState {
                            waiting: Some(ContainerStateWaiting { reason: Some(r), .. }),
                            ..
                        }) if is_error_reason(r) => return Some(to_error_message(r)),
                        Some(ContainerState {
                            terminated: Some(ContainerStateTerminated { reason: Some(r), .. }),
                            ..
                        }) if is_error_reason(r) => return Some(to_error_message(r)),
                        _ => {}
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn service_version(&self) -> Option<String> {
        let Some(annotations) = &self.metadata.annotations else {
            return None;
        };

        annotations.get("qovery.com/service-version").cloned()
    }
}

const DEFAULT_MAX_EVENTS: usize = 3;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Strategy {
    //AllEvents,
    OnlyWarningIfAny,
}

const WARNING_EVENT_TYPE: &str = "Warning";
pub fn get_last_events_for<'a>(
    events: impl Iterator<Item = &'a Event>,
    uid: &str,
    max_events: usize,
    strategy: Strategy,
) -> Vec<&'a Event> {
    let events = events
        .filter(|ev| ev.involved_object.uid.as_deref() == Some(uid))
        // last first
        .sorted_by(|evl, evr| evl.last_timestamp.cmp(&evr.last_timestamp).reverse())
        .take(max_events);

    match strategy {
        OnlyWarningIfAny => {
            // To avoid consuming the iterator
            if events.clone().any(|ev| ev.type_.as_deref() == Some(WARNING_EVENT_TYPE)) {
                events
                    .filter(|ev| ev.type_.as_deref() == Some(WARNING_EVENT_TYPE))
                    .collect()
            } else {
                events.collect()
            }
        }
    }
}
