use crate::utilities::to_short_id;
use itertools::Itertools;
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateTerminated, ContainerStateWaiting, Event, LoadBalancerStatus, PersistentVolumeClaim,
    Pod, PodStatus, Service, ServiceStatus,
};
use kube::api::ListParams;
use kube::Api;
use serde::Serialize;
use std::collections::HashMap;
use tera::Tera;
use uuid::Uuid;

#[derive(Debug)]
pub struct AppDeploymentInfo {
    pub id: Uuid,
    pub pods: Vec<Pod>,
    pub services: Vec<Service>,
    pub pvcs: Vec<PersistentVolumeClaim>,
    pub events: Vec<Event>,
}

#[derive(Debug, Serialize)]
pub enum DeploymentState {
    Starting,
    Ready,
    Terminating,
    Failing,
}

#[derive(Debug, Serialize)]
pub struct AppDeploymentRenderContext {
    pub name: String,
    pub commit: String,
    pub services: Vec<ServiceRenderContext>,
    pub nb_pods: usize,
    pub pods_failing: Vec<PodRenderContext>,
    pub pods_starting: Vec<PodRenderContext>,
    pub pods_terminating: Vec<PodRenderContext>,
    pub pvcs: Vec<PvcRenderContext>,
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

const PROGRESS_INFO_TEMPLATE: &str = r#"

Application at commit {{ commit }} deployment status report:
{%- for service in services %}
🔀 {{ service.type_ | capitalize }} {{ service.name }} is {{ service.state | upper }}: {{ service.message }}
{%- for event in service.events %}
 |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}

{% set all_pods = pods_failing | concat(with=pods_starting) -%}
🛰 Application has {{ nb_pods }} pods. {{ pods_starting | length }} starting, {{ pods_terminating | length }} terminating and {{ pods_failing | length }} in error
{%- for pod in all_pods %}
 |__ Pod {{ pod.name }} is {{ pod.state | upper }}: {{ pod.message }}
{%- for event in pod.events %}
    |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
{% for pvc in pvcs %}
💽 Network volume {{ pvc.name }} is {{ pvc.state | upper }}:
{%- for event in pvc.events %}
 |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor -%}"#;

pub async fn get_app_deployment_info(
    kube: &kube::Client,
    app_id: &Uuid,
    namespace: &str,
) -> Result<AppDeploymentInfo, kube::Error> {
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

    Ok(AppDeploymentInfo {
        id: *app_id,
        pods: pods.items,
        services: services.items,
        pvcs: pvcs.items,
        events: events.items,
    })
}

pub fn render_app_deployment_info(
    app_commit_id: &str,
    deployment_info: &AppDeploymentInfo,
) -> Result<String, tera::Error> {
    let services_ctx = to_services_render_context(&deployment_info.services, &deployment_info.events);
    let (pods_starting, pods_terminating, pods_failing) =
        to_pods_render_context(&deployment_info.pods, &deployment_info.events);
    let pvcs_ctx = to_pvc_render_context(&deployment_info.pvcs, &deployment_info.events);
    let render_ctx = AppDeploymentRenderContext {
        name: to_short_id(&deployment_info.id),
        commit: app_commit_id.to_string(),
        services: services_ctx,
        nb_pods: deployment_info.pods.len(),
        pods_failing,
        pods_starting,
        pods_terminating,
        pvcs: pvcs_ctx,
    };
    let ctx = tera::Context::from_serialize(render_ctx).unwrap();
    let mut tera = Tera::default();
    tera.register_filter("fmt_event_type", render_event_type);

    tera.render_str(PROGRESS_INFO_TEMPLATE, &ctx)
}

fn render_event_type(value: &tera::Value, _: &HashMap<String, tera::Value>) -> Result<tera::Value, tera::Error> {
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

fn to_event_context(ev: &Event) -> Option<EventRenderContext> {
    match (&ev.type_, &ev.message) {
        (None, _) | (_, None) => None,
        (Some(type_), Some(msg)) => Some(EventRenderContext {
            message: msg.replace('\n', ""),
            type_: type_.to_string(),
        }),
    }
}

fn to_services_render_context(services: &[Service], events: &[Event]) -> Vec<ServiceRenderContext> {
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

fn to_pods_render_context(
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

fn to_pvc_render_context(pvcs: &[PersistentVolumeClaim], events: &[Event]) -> Vec<PvcRenderContext> {
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

fn is_pod_in_error<'a>(pod: &'a Pod) -> Option<&'a str> {
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

#[cfg(test)]
mod test {
    use crate::deployment::deployment_info::{
        render_event_type, AppDeploymentRenderContext, DeploymentState, EventRenderContext, PodRenderContext,
        PvcRenderContext, ServiceRenderContext, PROGRESS_INFO_TEMPLATE,
    };
    use crate::utilities::to_short_id;
    use tera::Tera;
    use uuid::Uuid;

    #[test]
    fn test_application_rendering() {
        let app_id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let render_ctx = AppDeploymentRenderContext {
            name: to_short_id(&app_id),
            commit: "34645524c3221a596fb59e8dbad4381f10f93933".to_string(),
            services: vec![ServiceRenderContext {
                name: "app-z85ba6759".to_string(),
                type_: "Cloud load balancer".to_string(),
                state: DeploymentState::Starting,
                message: None, //Some("waiting to acquire an ip address".to_string()),
                events: vec![
                    EventRenderContext {
                        message: "No lease of ip yet".to_string(),
                        type_: "Normal".to_string(),
                    },
                    EventRenderContext {
                        message: "Pool of ip exhausted".to_string(),
                        type_: "Warning".to_string(),
                    },
                ],
            }],
            nb_pods: 6,
            pods_failing: vec![
                PodRenderContext {
                    name: "app-pod-1".to_string(),
                    state: DeploymentState::Failing,
                    message: Some("pod have been killed due to lack of/using too much memory resources".to_string()),
                    events: vec![],
                },
                PodRenderContext {
                    name: "app-pod-2".to_string(),
                    state: DeploymentState::Failing,
                    message: None,
                    events: vec![
                        EventRenderContext {
                            message: "Liveliness probe failed".to_string(),
                            type_: "Normal".to_string(),
                        },
                        EventRenderContext {
                            message: "Readiness probe failed".to_string(),
                            type_: "Warning".to_string(),
                        },
                    ],
                },
            ],
            pods_starting: vec![PodRenderContext {
                name: "app-pod-3".to_string(),
                state: DeploymentState::Starting,
                message: None,
                events: vec![
                    EventRenderContext {
                        message: "Pulling image :P".to_string(),
                        type_: "Normal".to_string(),
                    },
                    EventRenderContext {
                        message: "Container started".to_string(),
                        type_: "Warning".to_string(),
                    },
                ],
            }],
            pods_terminating: vec![PodRenderContext {
                name: "app-pod-4".to_string(),
                state: DeploymentState::Terminating,
                message: None,
                events: vec![],
            }],
            pvcs: vec![
                PvcRenderContext {
                name: "pvc-1212".to_string(),
                state: DeploymentState::Starting,
                events: vec![EventRenderContext {
                    message: "Failed to provision volume with StorageClass \"aws-ebs-io1-0\": InvalidParameterValue: The volume size is invalid for io1 volumes: 1 GiB. io1 volumes must be at least 4 GiB in size. Please specify a volume size above the minimum limit".to_string(),
                    type_: "Warning".to_string(),
                }],
            },
                PvcRenderContext {
                    name: "pvc-2121".to_string(),
                    state: DeploymentState::Ready,
                    events: vec![],
                }
            ],
        };

        let ctx = tera::Context::from_serialize(render_ctx).unwrap();
        let mut tera = Tera::default();
        tera.register_filter("fmt_event_type", render_event_type);

        let rendered_report = tera.render_str(PROGRESS_INFO_TEMPLATE, &ctx).unwrap();
        println!("{}", rendered_report);

        let gold_standard = r#"

Application at commit 34645524c3221a596fb59e8dbad4381f10f93933 deployment status report:
🔀 Cloud load balancer app-z85ba6759 is STARTING: 
 |__ ℹ️ No lease of ip yet
 |__ ⚠️ Pool of ip exhausted

🛰 Application has 6 pods. 1 starting, 1 terminating and 2 in error
 |__ Pod app-pod-1 is FAILING: pod have been killed due to lack of/using too much memory resources
 |__ Pod app-pod-2 is FAILING: 
    |__ ℹ️ Liveliness probe failed
    |__ ⚠️ Readiness probe failed
 |__ Pod app-pod-3 is STARTING: 
    |__ ℹ️ Pulling image :P
    |__ ⚠️ Container started

💽 Network volume pvc-1212 is STARTING:
 |__ ⚠️ Failed to provision volume with StorageClass "aws-ebs-io1-0": InvalidParameterValue: The volume size is invalid for io1 volumes: 1 GiB. io1 volumes must be at least 4 GiB in size. Please specify a volume size above the minimum limit
💽 Network volume pvc-2121 is READY:"#;

        assert_eq!(rendered_report, gold_standard);
    }
}
