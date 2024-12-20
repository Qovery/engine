use crate::environment::report::database::reporter::DatabaseDeploymentReport;
use crate::environment::report::utils::{
    get_tera_instance, to_pods_render_context_by_version, to_pvc_render_context, to_services_render_context,
    PodsRenderContext, PvcRenderContext, ServiceRenderContext,
};
use crate::infrastructure::models::cloud_provider::service::DatabaseType;
use crate::utilities::to_short_id;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DatabaseDeploymentRenderContext {
    pub name: String,
    pub is_managed: bool,
    pub type_: DatabaseType,
    pub version: String,
    pub services: Vec<ServiceRenderContext>,
    pub nb_pods: usize,
    pub pods_current_version: PodsRenderContext,
    pub pods_old_version: PodsRenderContext,
    pub pvcs: Vec<PvcRenderContext>,
}

const MANAGED_REPORT_TEMPLATE: &str = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Managed database {{ type_ }} v{{ version }} deployment is in progress ⏳, below the current status:
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
┃ ⛅️ Database instance is being provisionned at your cloud provider ...
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

const CONTAINER_REPORT_TEMPLATE: &str = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Container database {{ type_ }} v{{ version }} deployment is in progress ⏳, below the current status:
{%- for service in services %}
┃ 🔀 {{ service.type_ | capitalize }} {{ service.name }} is {{ service.state | upper }}
{%- if service.message %}
┃  |__ 💭 {{ service.message }}
{%- endif -%}
{%- for event in service.events %}
┃  |__ {{ event.type_ | fmt_event_type }} {{ event.message -}}
{%- endfor -%}
{%- endfor %}
┃
┃ 🛰 Database at old version has {{ pods_old_version.nb_pods }} pods: {{ pods_old_version.pods_running | length }} running, {{ pods_old_version.pods_starting | length }} starting, {{ pods_old_version.pods_terminating | length }} terminating and {{ pods_old_version.pods_failing | length }} in error
┃ 🛰 Database at new version {{ version }} has {{ pods_current_version.nb_pods }} pods: {{ pods_current_version.pods_running | length }} running, {{ pods_current_version.pods_starting | length }} starting, {{ pods_current_version.pods_terminating | length }} terminating and {{ pods_current_version.pods_failing | length }} in error
{%- set all_current_version_pods = pods_current_version.pods_failing | concat(with=pods_current_version.pods_starting) -%}
{%- for pod in all_current_version_pods %}
┃  |__ Pod {{ pod.name }} is {{ pod.state | upper }}
{%- if pod.message %}
┃     |__ 💭 {{ pod.message }}
{%- endif -%}
{%- for name, s in pod.container_states %}
{%- if s.restart_count > 0 %}
┃     |__ 💢 Container {{ name }} crashed {{ s.restart_count }} times. Last terminated with exit code {{ s.last_state.exit_code }} due to {{ s.last_state.reason }} {{ s.last_state.message }} at {{ s.last_state.finished_at }}
{%- if s.last_state.exit_code_msg %}
┃     |__ 💭 Exit code {{ s.last_state.exit_code }} means {{ s.last_state.exit_code_msg }}
{%- endif -%}
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
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

pub(crate) fn render_database_deployment_report(
    deployment_report: &DatabaseDeploymentReport,
) -> Result<String, tera::Error> {
    let services_ctx = to_services_render_context(&deployment_report.services, &deployment_report.events);
    let (pods_current_version, pods_old_version) = to_pods_render_context_by_version(
        &deployment_report.pods,
        &deployment_report.events,
        &deployment_report.version,
    );
    let pvcs_ctx = to_pvc_render_context(&deployment_report.pvcs, &deployment_report.events);
    let render_ctx = DatabaseDeploymentRenderContext {
        name: to_short_id(&deployment_report.id),
        is_managed: deployment_report.is_managed,
        type_: deployment_report.type_,
        version: deployment_report.version.clone(),
        services: services_ctx,
        nb_pods: deployment_report.pods.len(),
        // hack db does not have the annotation for version :x
        // So everything is in the old version
        pods_old_version: pods_current_version,
        pods_current_version: pods_old_version,
        pvcs: pvcs_ctx,
    };

    let ctx = tera::Context::from_serialize(render_ctx)?;
    let report_template = if deployment_report.is_managed {
        MANAGED_REPORT_TEMPLATE
    } else {
        CONTAINER_REPORT_TEMPLATE
    };

    get_tera_instance().render_str(report_template, &ctx)
}

#[cfg(test)]
mod test {
    use crate::environment::report::database::renderer::{
        DatabaseDeploymentRenderContext, CONTAINER_REPORT_TEMPLATE, MANAGED_REPORT_TEMPLATE,
    };
    use crate::environment::report::utils::{
        exit_code_to_msg, get_tera_instance, DeploymentState, EventRenderContext, PodRenderContext, PodsRenderContext,
        PvcRenderContext, QContainerState, QContainerStateTerminated, ServiceRenderContext,
    };
    use crate::infrastructure::models::cloud_provider::service::DatabaseType;
    use crate::utilities::to_short_id;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1;
    use maplit::btreemap;
    use uuid::Uuid;

    #[test]
    fn test_db_container_rendering() {
        let app_id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let render_ctx = DatabaseDeploymentRenderContext {
            name: to_short_id(&app_id),
            is_managed: false,
            type_: DatabaseType::PostgreSQL,
            version: "14".to_string(),
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
            pods_old_version: PodsRenderContext {
                nb_pods: 0,
                pods_running: vec![],
                pods_starting: vec![],
                pods_failing: vec![],
                pods_terminating: vec![],
            },
            pods_current_version: PodsRenderContext {
                nb_pods: 4,
                pods_failing: vec![
                    PodRenderContext {
                        name: "app-pod-1".to_string(),
                        state: DeploymentState::Failing,
                        message: Some("pod have been killed due to lack of/using too much memory resources".to_string()),
                        container_states: btreemap! {
                        "app-container-1".to_string() => QContainerState { restart_count: 0u32, last_state: QContainerStateTerminated::default() },
                    },
                        events: vec![],
                        service_version: None,
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
                        service_version: None,
                    },
                ],
                pods_starting: vec![PodRenderContext {
                    name: "app-pod-3".to_string(),
                    state: DeploymentState::Starting,
                    message: None,
                    container_states: btreemap! {
                        "app-container-1".to_string() => QContainerState {
                        restart_count: 3u32,
                        last_state: QContainerStateTerminated {
                                exit_code: 132,
                                exit_code_msg: exit_code_to_msg(132),
                                reason:  Some("OOMKilled".to_string()),
                                message: Some("using too much memory".to_string()),
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
                    service_version: None,
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
                pods_running: vec![],
            },
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
        let rendered_report = get_tera_instance().render_str(CONTAINER_REPORT_TEMPLATE, &ctx).unwrap();
        println!("{rendered_report}");

        let gold_standard = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Container database PostgreSQL v14 deployment is in progress ⏳, below the current status:
┃ 🔀 Cloud load balancer app-z85ba6759 is STARTING
┃  |__ ℹ️ No lease of ip yet
┃  |__ ⚠️ Pool of ip exhausted
┃
┃ 🛰 Database at old version has 0 pods: 0 running, 0 starting, 0 terminating and 0 in error
┃ 🛰 Database at new version 14 has 4 pods: 0 running, 1 starting, 1 terminating and 2 in error
┃  |__ Pod app-pod-1 is FAILING
┃     |__ 💭 pod have been killed due to lack of/using too much memory resources
┃  |__ Pod app-pod-2 is FAILING
┃     |__ ℹ️ Liveliness probe failed
┃     |__ ⚠️ Readiness probe failed
┃  |__ Pod app-pod-3 is STARTING
┃     |__ 💢 Container app-container-1 crashed 3 times. Last terminated with exit code 132 due to OOMKilled using too much memory at 1970-01-01T00:00:00Z
┃     |__ ℹ️ Pulling image :P
┃     |__ ⚠️ Container started
┃
┃ 💽 Network volume pvc-1212 is STARTING
┃  |__ ⚠️ Failed to provision volume with StorageClass "aws-ebs-io1-0": InvalidParameterValue: The volume size is invalid for io1 volumes: 1 GiB. io1 volumes must be at least 4 GiB in size. Please specify a volume size above the minimum limit
┃ 💽 Network volume pvc-2121 is READY
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

        for (rendered_line, gold_line) in rendered_report.lines().zip(gold_standard.lines()) {
            assert_eq!(rendered_line.trim_end(), gold_line);
        }
    }

    #[test]
    fn test_db_managed_rendering() {
        let app_id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let render_ctx = DatabaseDeploymentRenderContext {
            name: to_short_id(&app_id),
            is_managed: false,
            type_: DatabaseType::PostgreSQL,
            version: "13".to_string(),
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
            nb_pods: 0,
            pods_current_version: PodsRenderContext {
                nb_pods: 0,
                pods_running: vec![],
                pods_starting: vec![],
                pods_failing: vec![],
                pods_terminating: vec![],
            },
            pods_old_version: PodsRenderContext {
                nb_pods: 0,
                pods_running: vec![],
                pods_starting: vec![],
                pods_failing: vec![],
                pods_terminating: vec![],
            },
            pvcs: vec![],
        };

        let ctx = tera::Context::from_serialize(render_ctx).unwrap();
        let rendered_report = get_tera_instance().render_str(MANAGED_REPORT_TEMPLATE, &ctx).unwrap();
        println!("{rendered_report}");

        let gold_standard = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Managed database PostgreSQL v13 deployment is in progress ⏳, below the current status:
┃ 🔀 Cloud load balancer app-z85ba6759 is STARTING
┃  |__ ℹ️ No lease of ip yet
┃  |__ ⚠️ Pool of ip exhausted
┃
┃ ⛅️ Database instance is being provisionned at your cloud provider ...
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

        for (rendered_line, gold_line) in rendered_report.lines().zip(gold_standard.lines()) {
            assert_eq!(rendered_line.trim_end(), gold_line);
        }
    }
}
