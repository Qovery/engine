use itertools::Itertools;
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateTerminated, ContainerStateWaiting, Event, LoadBalancerStatus, PersistentVolumeClaim,
    Pod, PodStatus, Service, ServiceStatus,
};
use kube::api::ListParams;
use kube::Api;
use uuid::Uuid;

#[derive(Debug)]
pub struct AppDeploymentInfo {
    pub id: Uuid,
    pub pods: Vec<Pod>,
    pub services: Vec<Service>,
    pub pvc: Vec<PersistentVolumeClaim>,
    pub events: Vec<Event>,
}

pub async fn get_app_deployment_info(
    kube: &kube::Client,
    app_id: &Uuid,
    namespace: &str,
) -> Result<AppDeploymentInfo, kube::Error> {
    let selector = format!("appLongId={}", app_id);
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

    Ok(AppDeploymentInfo {
        id: *app_id,
        pods: pods.items,
        services: services.items,
        pvc: pvcs.items,
        events: events.items,
    })
}

pub fn format_app_deployment_info(deployment_info: &AppDeploymentInfo) -> Vec<String> {
    let mut msgs: Vec<String> = vec![];

    msgs.extend(format_service(&deployment_info.services, &deployment_info.events));
    msgs.extend(format_pods(&deployment_info.id, &deployment_info.pods, &deployment_info.events));
    // TODO: Add pvc

    msgs
}

fn format_service(services: &[Service], events: &[Event]) -> Vec<String> {
    let mut msgs: Vec<String> = Vec::with_capacity(services.len().max(1));
    let format_event_message = |ev: &Event| -> Option<String> {
        match (&ev.type_, &ev.message) {
            (None, _) | (_, None) => None,
            (Some(type_), Some(msg)) if type_ == "Normal" => Some(msg.to_string()),
            (Some(type_), Some(msg)) if type_ == "Warning" => Some(format!("⚠️ {} ⚠️", msg)),
            (Some(_), Some(msg)) => Some(format!("💢️ {} 💢️", msg)),
        }
    };

    if services.is_empty() {
        msgs.push("🔀 No Load balancer exist".to_string());
    }

    for svc in services {
        // extract values
        let spec = if let Some(spec) = &svc.spec { spec } else { continue };
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
            let event_msgs = get_last_events_for(events.iter(), svc_uid, 5)
                .flat_map(format_event_message)
                .join(", ");
            msgs.push(format!("🔀 Load balancer `{}` is being deleted: {}", svc_name, event_msgs));
            continue;
        }

        // If we have a cluster ip, it is an internal service
        let svc_type: &str = spec.type_.as_deref().unwrap_or("");

        // if it is a load balancer we need to check that it has an external ip/name assigned
        if svc_type == "LoadBalancer" {
            match &svc.status {
                Some(ServiceStatus {
                    load_balancer: Some(LoadBalancerStatus { ingress: Some(ingress) }),
                    ..
                }) if !ingress.is_empty() => {
                    msgs.push(format!("🔀 Load balancer `{}` is ready", svc_name));
                }
                _ => {
                    // no ip for the LB
                    let event_msgs = get_last_events_for(events.iter(), svc_uid, 5)
                        .flat_map(format_event_message)
                        .join(", ");
                    msgs.push(format!(
                        "🔀 Load balancer `{}` is waiting to be assigned an Ip: {}",
                        svc_name, event_msgs
                    ));
                }
            }

            continue;
        }

        // If it is not an LB (i.e: ClusterIP), we just check it has an internal ip assigned
        if spec.cluster_ips.as_ref().map_or(0, |ips| ips.len()) > 0 {
            msgs.push(format!("🔀 Load balancer `{}` is ready", svc_name));
            continue;
        }

        // no ip for the LB
        let event_msgs = get_last_events_for(events.iter(), svc_uid, 5)
            .flat_map(format_event_message)
            .join(", ");
        msgs.push(format!(
            "🔀 Load balancer `{}` is waiting to be assigned an Ip: {}",
            svc_name, event_msgs
        ));

        continue;
    }

    msgs
}

fn format_pods(app_id: &Uuid, pods: &[Pod], events: &[Event]) -> Vec<String> {
    let mut msgs: Vec<String> = Vec::with_capacity(pods.len().max(1));
    let app_id = app_id.to_string();

    let mut nb_pods_in_deletion = 0;
    let mut pods_in_error: Vec<(&Pod, &str)> = Vec::with_capacity(pods.len());
    let mut pods_starting: Vec<&Pod> = Vec::with_capacity(pods.len());

    for pod in pods {
        if pod.metadata.deletion_timestamp.is_some() {
            nb_pods_in_deletion += 1;
            continue;
        }

        if let Some(error_reason) = is_pod_in_error(pod) {
            pods_in_error.push((pod, error_reason));
            continue;
        }

        if is_pod_starting(pod) {
            pods_starting.push(pod);
            continue;
        }
    }

    msgs.push(format!(
        "🛰 Application `{}` has {} pods. {} are starting, {} are terminating and {} are in error",
        app_id,
        pods.len(),
        pods_starting.len(),
        nb_pods_in_deletion,
        pods_in_error.len()
    ));

    // Display more information for pods that are in error
    for (pod, error_reason) in pods_in_error {
        let pod_name = pod.metadata.name.as_deref().unwrap_or("");
        let pod_uid = pod.metadata.uid.as_deref().unwrap_or("");
        let last_event: &str = get_last_events_for(events.iter(), pod_uid, 1)
            .next()
            .and_then(|e| e.message.as_deref())
            .unwrap_or("");

        let msg = format!("    Pod `{}` is in error due to `{}`: {}", pod_name, error_reason, last_event);
        msgs.push(msg);
    }

    // Display more information for pods that are starting
    for pod in pods_starting {
        let pod_name = pod.metadata.name.as_deref().unwrap_or("");
        let pod_uid = pod.metadata.uid.as_deref().unwrap_or("");
        let last_event: &str = get_last_events_for(events.iter(), pod_uid, 1)
            .next()
            .and_then(|e| e.message.as_deref())
            .unwrap_or("");

        let msg = format!("    Pod `{}` starting because not yet ready: `{}`", pod_name, last_event,);
        msgs.push(msg);
    }

    msgs
}

fn is_pod_in_error(pod: &Pod) -> Option<&str> {
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
                    }) if is_error_reason(r) => return Some(r),
                    Some(ContainerState {
                        terminated: Some(ContainerStateTerminated { reason: Some(r), .. }),
                        ..
                    }) if is_error_reason(r) => return Some(r),
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

fn is_pod_starting(pod: &Pod) -> bool {
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

fn get_last_events_for<'a>(
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
