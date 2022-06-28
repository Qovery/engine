use itertools::Itertools;
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateTerminated, ContainerStateWaiting, Event, LoadBalancerStatus, PersistentVolumeClaim,
    Pod, PodStatus, Service, ServiceStatus,
};
use serde::Serialize;
use std::collections::HashMap;
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
pub struct PodRenderContext {
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
                events: get_last_events_for(events.iter(), svc_uid, 2)
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
                        message: None,
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
                        events: get_last_events_for(events.iter(), svc_uid, 2)
                            .flat_map(to_event_context)
                            .collect(),
                    });
                }
            }

            continue;
        }

        // If it is not an LB (i.e: ClusterIP), we just check it has an internal ip assigned
        if spec.cluster_ips.as_ref().map_or(0, |ips| ips.len()) > 0 {
            svc_ctx.push(ServiceRenderContext {
                name: svc_name.to_string(),
                type_: "kubernetes load balancer".to_string(),
                state: DeploymentState::Ready,
                message: None,
                events: vec![],
            });
            continue;
        }

        // no ip for the LB
        svc_ctx.push(ServiceRenderContext {
            name: svc_name.to_string(),
            type_: "kubernetes load balancer".to_string(),
            state: DeploymentState::Starting,
            message: Some("waiting to be assigned an Ip".to_string()),
            events: get_last_events_for(events.iter(), svc_uid, 2)
                .flat_map(to_event_context)
                .collect(),
        });

        continue;
    }

    svc_ctx
}

pub fn to_pods_render_context(
    pods: &[Pod],
    events: &[Event],
) -> (Vec<PodRenderContext>, Vec<PodRenderContext>, Vec<PodRenderContext>) {
    if pods.is_empty() {
        return (vec![], vec![], vec![]);
    }

    let mut pods_failing: Vec<PodRenderContext> = Vec::with_capacity(pods.len());
    let mut pods_starting: Vec<PodRenderContext> = Vec::with_capacity(pods.len());
    let mut pods_terminating: Vec<PodRenderContext> = Vec::with_capacity(pods.len());

    for pod in pods {
        let pod_name = pod.metadata.name.as_deref().unwrap_or("");
        let pod_uid = pod.metadata.uid.as_deref().unwrap_or("");

        if pod.metadata.deletion_timestamp.is_some() {
            pods_terminating.push(PodRenderContext {
                name: pod_name.to_string(),
                state: DeploymentState::Terminating,
                message: None,
                events: vec![],
            });
            continue;
        }

        if let Some(error_reason) = is_pod_in_error(pod) {
            pods_failing.push(PodRenderContext {
                name: pod_name.to_string(),
                state: DeploymentState::Failing,
                message: Some(error_reason.to_string()),
                events: get_last_events_for(events.iter(), pod_uid, 2)
                    .flat_map(to_event_context)
                    .collect(),
            });
            continue;
        }

        if is_pod_starting(pod) {
            pods_starting.push(PodRenderContext {
                name: pod_name.to_string(),
                state: DeploymentState::Starting,
                message: None,
                events: get_last_events_for(events.iter(), pod_uid, 2)
                    .flat_map(to_event_context)
                    .collect(),
            });
            continue;
        }
    }

    (pods_starting, pods_terminating, pods_failing)
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
                events: get_last_events_for(events.iter(), pvc_uid, 2)
                    .flat_map(to_event_context)
                    .collect(),
            });
            continue;
        }

        if is_starting {
            pvcs_context.push(PvcRenderContext {
                name: pvc_name.to_string(),
                state: DeploymentState::Starting,
                events: get_last_events_for(events.iter(), pvc_uid, 2)
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

pub fn is_pod_in_error<'a>(pod: &'a Pod) -> Option<&'a str> {
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
            "OOMKilled" => "OOM killed, pod have been killed due to lack of/using too much memory resources",
            "CrashLoopBackOff" => "crash loop, pod is restarting too frequently. Look into your application logs",
            "ErrImagePull" => "cannot pull the image for your container",
            "ImagePullBackOff" => "cannot pull the image for your container",
            "Error" => "an undefined error occurred. Look into your applications logs and message below",
            _ => reason,
        }
    };

    // We need to loop over all status of each container in the pod in order to know
    // if there is something fishy or not, not really friendly...
    match pod.status.as_ref() {
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

pub fn is_pod_starting(pod: &Pod) -> bool {
    // If the pod is in pending phase, it means it starts
    if let Some("Pending") = pod.status.as_ref().and_then(|x| x.phase.as_deref()) {
        return true;
    }

    let conditions = match &pod.status {
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

pub fn get_last_events_for<'a>(
    events: impl Iterator<Item = &'a Event>,
    uid: &str,
    max_events: usize,
) -> impl Iterator<Item = &'a Event> {
    let oldest_event = chrono::Utc::now() - chrono::Duration::minutes(2);
    events
        // keep only selected object and events that are older than above time (2min)
        .filter(|ev| {
            ev.involved_object.uid.as_deref() == Some(uid)
                && ev.last_timestamp.as_ref().map_or(false, |t| t.0 > oldest_event)
        })
        // last first
        .sorted_by(|evl, evr| evl.last_timestamp.cmp(&evr.last_timestamp).reverse())
        .take(max_events)
}
