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
    pub commit: String,
    pub services: Vec<ServiceRenderContext>,
    pub nb_pods: usize,
    pub pods_failing: Vec<PodRenderContext>,
    pub pods_starting: Vec<PodRenderContext>,
    pub pods_terminating: Vec<PodRenderContext>,
    pub pvcs: Vec<PvcRenderContext>,
}

const REPORT_TEMPLATE: &str = r#"
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Application at commit {{ commit }} deployment status report:
{%- for service in services %}
┃ 🔀 {{ service.type_ | capitalize }} {{ service.name }} is {{ service.state | upper }} {{ service.message }}
{%- for event in service.events %}
┃  |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
┃
{% set all_pods = pods_failing | concat(with=pods_starting) -%}
┃ 🛰 Application has {{ nb_pods }} pods. {{ pods_starting | length }} starting, {{ pods_terminating | length }} terminating and {{ pods_failing | length }} in error
{%- for pod in all_pods %}
┃  |__ Pod {{ pod.name }} is {{ pod.state | upper }} {{ pod.message }}{%- if pod.restart_count > 0 %}
┃     |__ 💢 Pod crashed {{ pod.restart_count }} times
{%- endif -%}
{%- for event in pod.events %}
┃     |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
┃
{%- for pvc in pvcs %}
┃ 💽 Network volume {{ pvc.name }} is {{ pvc.state | upper }}
{%- for event in pvc.events %}
┃  |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
┃
┃ ⛑ Need Help ? Please consult our FAQ in order to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

pub(super) fn render_app_deployment_report(
    app_commit_id: &str,
    deployment_info: &AppDeploymentReport,
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
    let ctx = tera::Context::from_serialize(render_ctx)?;
    get_tera_instance().render_str(REPORT_TEMPLATE, &ctx)
}

#[cfg(test)]
mod test {
    use crate::deployment_report::application::renderer::{
        AppDeploymentRenderContext, ServiceRenderContext, REPORT_TEMPLATE,
    };
    use crate::deployment_report::utils::{
        fmt_event_type, DeploymentState, EventRenderContext, PodRenderContext, PvcRenderContext,
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
                    restart_count: 5,
                    events: vec![],
                },
                PodRenderContext {
                    name: "app-pod-2".to_string(),
                    state: DeploymentState::Failing,
                    message: None,
                    restart_count: 0,
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
                restart_count: 1,
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
                restart_count: 0,
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
        tera.register_filter("fmt_event_type", fmt_event_type);

        let rendered_report = tera.render_str(REPORT_TEMPLATE, &ctx).unwrap();
        println!("{}", rendered_report);

        let gold_standard = r#"
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Application at commit 34645524c3221a596fb59e8dbad4381f10f93933 deployment status report:
┃ 🔀 Cloud load balancer app-z85ba6759 is STARTING
┃  |__ ℹ️ No lease of ip yet
┃  |__ ⚠️ Pool of ip exhausted
┃
┃ 🛰 Application has 6 pods. 1 starting, 1 terminating and 2 in error
┃  |__ Pod app-pod-1 is FAILING pod have been killed due to lack of/using too much memory resources
┃     |__ 💢 Pod crashed 5 times
┃  |__ Pod app-pod-2 is FAILING
┃     |__ ℹ️ Liveliness probe failed
┃     |__ ⚠️ Readiness probe failed
┃  |__ Pod app-pod-3 is STARTING
┃     |__ 💢 Pod crashed 1 times
┃     |__ ℹ️ Pulling image :P
┃     |__ ⚠️ Container started
┃
┃ 💽 Network volume pvc-1212 is STARTING
┃  |__ ⚠️ Failed to provision volume with StorageClass "aws-ebs-io1-0": InvalidParameterValue: The volume size is invalid for io1 volumes: 1 GiB. io1 volumes must be at least 4 GiB in size. Please specify a volume size above the minimum limit
┃ 💽 Network volume pvc-2121 is READY
┃
┃ ⛑ Need Help ? Please consult our FAQ in order to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

        for (rendered_line, gold_line) in rendered_report.lines().zip(gold_standard.lines()) {
            assert_eq!(rendered_line.trim_end(), gold_line);
        }
    }
}
