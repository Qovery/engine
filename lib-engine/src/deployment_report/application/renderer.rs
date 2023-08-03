use crate::cloud_provider::service::ServiceType;
use crate::deployment_report::application::reporter::AppDeploymentReport;
use crate::deployment_report::utils::{
    get_tera_instance, to_pods_render_context, to_pvc_render_context, to_services_render_context, PodRenderContext,
    PvcRenderContext, ServiceRenderContext,
};
use crate::utilities::to_short_id;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AppDeploymentRenderContext {
    pub name: String,
    pub service_type: String,
    pub tag_name: String,
    pub tag: String,
    pub services: Vec<ServiceRenderContext>,
    pub nb_pods: usize,
    pub pods_failing: Vec<PodRenderContext>,
    pub pods_starting: Vec<PodRenderContext>,
    pub pods_terminating: Vec<PodRenderContext>,
    pub pvcs: Vec<PvcRenderContext>,
}

const REPORT_TEMPLATE: &str = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ {{ service_type }} at {{ tag_name }} {{ tag }} deployment is in progress ⏳, below the current status:
{%- for service in services %}
┃ 🔀 {{ service.type_ | capitalize }} {{ service.name }} is {{ service.state | upper }}
{%- if service.message %}
┃  |__ 💭 {{ service.message }}
{%- endif -%}
{%- for event in service.events %}
┃  |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
┃
{% set all_pods = pods_failing | concat(with=pods_starting) -%}
┃ 🛰 {{ service_type }} has {{ nb_pods }} pods. {{ pods_starting | length }} starting, {{ pods_terminating | length }} terminating and {{ pods_failing | length }} in error
{%- for pod in all_pods %}
┃  |__ Pod {{ pod.name }} at commit/tag {{ pod.service_version }} is {{ pod.state | upper }} {{ pod.message }}
{%- for name, s in pod.container_states %}
{%- if s.restart_count > 0 %}
┃     |__ 💢 Container {{ name }} crashed {{ s.restart_count }} times. Last terminated with exit code {{ s.last_state.exit_code }} due to {{ s.last_state.reason }} {{ s.last_state.message }} at {{ s.last_state.finished_at }}
{%- endif -%}
{%- endfor -%}
{%- for event in pod.events %}
┃     |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
{%- if pvcs %}
┃
{%- for pvc in pvcs %}
┃ 💽 Network volume {{ pvc.name }} is {{ pvc.state | upper }}
{%- for event in pvc.events %}
┃  |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
{%- endif %}
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

pub(super) fn render_app_deployment_report(
    service_type: ServiceType,
    service_tag: &str,
    deployment_info: &AppDeploymentReport,
) -> Result<String, tera::Error> {
    let services_ctx = to_services_render_context(&deployment_info.services, &deployment_info.events);
    let (pods_starting, pods_terminating, pods_failing, _) =
        to_pods_render_context(&deployment_info.pods, &deployment_info.events);
    let pvcs_ctx = to_pvc_render_context(&deployment_info.pvcs, &deployment_info.events);
    let render_ctx = AppDeploymentRenderContext {
        name: to_short_id(&deployment_info.id),
        service_type: service_type.to_string(),
        tag_name: if service_type == ServiceType::Application {
            "commit"
        } else {
            "tag"
        }
        .to_string(),
        tag: service_tag.to_string(),
        services: services_ctx,
        nb_pods: deployment_info.pods.len(),
        pods_failing,
        pods_starting,
        pods_terminating,
        pvcs: pvcs_ctx,
    };
    let ctx = tera::Context::from_serialize(render_ctx)?;
    get_tera_instance().render_str(REPORT_TEMPLATE, &ctx)
}

#[cfg(test)]
mod test {
    use crate::cloud_provider::service::ServiceType;
    use crate::deployment_report::application::renderer::{
        AppDeploymentRenderContext, ServiceRenderContext, REPORT_TEMPLATE,
    };
    use crate::deployment_report::utils::{
        fmt_event_type, DeploymentState, EventRenderContext, PodRenderContext, PvcRenderContext, QContainerState,
        QContainerStateTerminated,
    };
    use crate::utilities::to_short_id;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1;
    use maplit::btreemap;
    use tera::Tera;
    use uuid::Uuid;

    #[test]
    fn test_application_rendering() {
        let app_id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let render_ctx = AppDeploymentRenderContext {
            name: to_short_id(&app_id),
            service_type: ServiceType::Application.to_string(),
            tag_name: "commit".to_string(),
            tag: "34645524c3221a596fb59e8dbad4381f10f93933".to_string(),
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
                    container_states: btreemap! {
                        "app-container-1".to_string() => QContainerState {
                            restart_count: 5u32,
                            last_state: QContainerStateTerminated {
                                exit_code: 132,
                                reason:  Some("OOMKilled".to_string()),
                                message: Some("using too much memory".to_string()),
                                finished_at: Some(v1::Time(chrono::DateTime::default())),
                            }
                        },
                    },
                    service_version: Some("debian:bookworm-slim".to_string()),
                },
                PodRenderContext {
                    name: "app-pod-2".to_string(),
                    state: DeploymentState::Failing,
                    message: None,
                    container_states: btreemap! {
                        "app-container-1".to_string() => QContainerState { restart_count: 0u32, last_state: QContainerStateTerminated::default() },
                    },
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
                    service_version: Some("e3c9b8b158e91229ab3f45d306f818feb2e564c3".to_string()),
                },
            ],
            pods_starting: vec![PodRenderContext {
                name: "app-pod-3".to_string(),
                state: DeploymentState::Starting,
                message: None,
                container_states: btreemap! {
                        "app-container-1".to_string() => QContainerState {
                            restart_count: 1u32,
                            last_state: QContainerStateTerminated {
                                exit_code: 132,
                                reason:  Some("Error".to_string()),
                                message: None,
                                finished_at: Some(v1::Time(chrono::DateTime::default())),
                            }
                        },
                    },
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
                service_version: Some("AKA 47".to_string()),
            }],
            pods_terminating: vec![PodRenderContext {
                name: "app-pod-4".to_string(),
                state: DeploymentState::Terminating,
                message: None,
                container_states: btreemap! {
                        "app-container-1".to_string() => QContainerState { restart_count: 0u32, last_state: QContainerStateTerminated::default() },
                    },
                events: vec![],
                service_version: None,
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
        tera.register_filter("fmt_event_type", fmt_event_type);

        let rendered_report = tera.render_str(REPORT_TEMPLATE, &ctx).unwrap();
        println!("{rendered_report}");

        let gold_standard = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Application at commit 34645524c3221a596fb59e8dbad4381f10f93933 deployment is in progress ⏳, below the current status:
┃ 🔀 Cloud load balancer app-z85ba6759 is STARTING
┃  |__ ℹ️ No lease of ip yet
┃  |__ ⚠️ Pool of ip exhausted
┃
┃ 🛰 Application has 6 pods. 1 starting, 1 terminating and 2 in error
┃  |__ Pod app-pod-1 at commit/tag debian:bookworm-slim is FAILING pod have been killed due to lack of/using too much memory resources
┃     |__ 💢 Container app-container-1 crashed 5 times. Last terminated with exit code 132 due to OOMKilled using too much memory at 1970-01-01T00:00:00Z
┃  |__ Pod app-pod-2 at commit/tag e3c9b8b158e91229ab3f45d306f818feb2e564c3 is FAILING
┃     |__ ℹ️ Liveliness probe failed
┃     |__ ⚠️ Readiness probe failed
┃  |__ Pod app-pod-3 at commit/tag AKA 47 is STARTING
┃     |__ 💢 Container app-container-1 crashed 1 times. Last terminated with exit code 132 due to Error  at 1970-01-01T00:00:00Z
┃     |__ ℹ️ Pulling image :P
┃     |__ ⚠️ Container started
┃
┃ 💽 Network volume pvc-1212 is STARTING
┃  |__ ⚠️ Failed to provision volume with StorageClass "aws-ebs-io1-0": InvalidParameterValue: The volume size is invalid for io1 volumes: 1 GiB. io1 volumes must be at least 4 GiB in size. Please specify a volume size above the minimum limit
┃ 💽 Network volume pvc-2121 is READY
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

        for (rendered_line, gold_line) in rendered_report.lines().zip(gold_standard.lines()) {
            assert_eq!(rendered_line.trim_end(), gold_line);
        }
    }
}
