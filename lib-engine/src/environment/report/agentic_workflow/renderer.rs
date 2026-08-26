use serde::Serialize;

use crate::environment::report::agentic_workflow::reporter::AgenticWorkflowDeploymentReport;
use crate::environment::report::utils::{
    JobRenderContext, PodsRenderContext, get_tera_instance, to_job_render_context, to_pods_render_context_by_state,
};
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::utilities::to_short_id;

#[derive(Debug, Serialize)]
pub struct AgenticWorkflowDeploymentRenderContext {
    pub name: String,
    pub action: String,
    pub job: Option<JobRenderContext>,
    pub pods: PodsRenderContext,
}

const REPORT_TEMPLATE: &str = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ {{ action }} of agentic workflow {{ name }} is in progress ⏳, below the current status:
┃
┃ 🛰 Agentic workflow has {{ pods.nb_pods }} pods: {{ pods.pods_running | length }} running, {{ pods.pods_starting | length }} starting, {{ pods.pods_terminating | length }} terminating and {{ pods.pods_failing | length }} in error
{%- set pods_to_detail = pods.pods_failing | concat(with=pods.pods_starting) -%}
{%- if job %}
┃  |__ Job {{ job.name }}
{%- for event in job.events %}
┃     |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endif -%}
{%- for pod in pods_to_detail %}
┃  |__ Pod {{ pod.name }} is {{ pod.state | upper }}
{%- if pod.message %}
┃     |__ 💭 {{ pod.message }}
{%- endif -%}
{%- for name, s in pod.container_states %}
{%- if s.restart_count > 0 %}
┃     |__ 💢 Container {{ name }} crashed {{ s.restart_count }} times. Last terminated with exit code {{ s.last_state.exit_code }} due to {{ s.last_state.reason }} {{ s.last_state.message }} at {{ s.last_state.finished_at }}
{%- if s.last_state.exit_code_msg %}
┃     |__ 💭 Exit code {{ s.last_state.exit_code }} means {{ s.last_state.exit_code_msg }}
{%- endif -%}
{%- endif -%}
{%- endfor -%}
{%- for event in pod.events %}
┃     |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

pub(crate) fn render_agentic_workflow_deployment_report(
    action: &Action,
    deployment_info: &AgenticWorkflowDeploymentReport,
) -> Result<String, tera::Error> {
    // Agentic workflows have no version/tag, so pods cannot be split between old and new
    // version like for a job. Group them by state instead.
    let (pods_starting, pods_terminating, pods_failing, pods_running) =
        to_pods_render_context_by_state(&deployment_info.pods, &deployment_info.events);
    let job_ctx = deployment_info
        .job
        .as_ref()
        .map(|job| to_job_render_context(job, &deployment_info.events));

    let render_ctx = AgenticWorkflowDeploymentRenderContext {
        name: to_short_id(&deployment_info.id),
        action: action.to_string(),
        job: job_ctx,
        pods: PodsRenderContext {
            nb_pods: deployment_info.pods.len(),
            pods_running,
            pods_starting,
            pods_terminating,
            pods_failing,
        },
    };
    let ctx = tera::Context::from_serialize(render_ctx)?;
    get_tera_instance().render_str(REPORT_TEMPLATE, &ctx)
}

#[cfg(test)]
mod test {
    use k8s_openapi::apimachinery::pkg::apis::meta::v1;
    use maplit::btreemap;
    use tera::Tera;
    use uuid::Uuid;

    use crate::environment::report::utils::{
        DeploymentState, EventRenderContext, PodRenderContext, QContainerState, QContainerStateTerminated,
        exit_code_to_msg, fmt_event_type,
    };
    use crate::utilities::to_short_id;

    use super::*;

    #[test]
    fn test_agentic_workflow_rendering() {
        let service_id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let render_ctx = AgenticWorkflowDeploymentRenderContext {
            name: to_short_id(&service_id),
            action: Action::Create.to_string(),
            job: Some(JobRenderContext {
                name: "agentic-workflow-z5a0dd39e".to_string(),
                state: DeploymentState::Starting,
                message: None,
                events: vec![EventRenderContext {
                    message: "Error creating: pods \"agentic-workflow-z5a0dd39e-\" is forbidden".to_string(),
                    type_: "Warning".to_string(),
                }],
            }),
            pods: PodsRenderContext {
                nb_pods: 1,
                pods_running: vec![],
                pods_starting: vec![],
                pods_terminating: vec![],
                pods_failing: vec![PodRenderContext {
                    name: "agentic-workflow-pod-1".to_string(),
                    state: DeploymentState::Failing,
                    message: Some("Pod have been killed due to lack of/using too much memory resources".to_string()),
                    container_states: btreemap! {
                        "agent".to_string() => QContainerState {
                            restart_count: 5u32,
                            last_state: QContainerStateTerminated {
                                exit_code: 137,
                                exit_code_msg: exit_code_to_msg(137),
                                reason: Some("OOMKilled".to_string()),
                                message: Some("using too much memory".to_string()),
                                finished_at: Some(v1::Time(k8s_openapi::jiff::Timestamp::UNIX_EPOCH)),
                            }
                        },
                    },
                    events: vec![],
                    service_version: None,
                }],
            },
        };

        let ctx = tera::Context::from_serialize(render_ctx).unwrap();
        let mut tera = Tera::default();
        tera.register_filter("fmt_event_type", fmt_event_type);

        let rendered_report = tera.render_str(REPORT_TEMPLATE, &ctx).unwrap();
        println!("{rendered_report}");

        let gold_standard = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Deployment of agentic workflow z123e4567 is in progress ⏳, below the current status:
┃
┃ 🛰 Agentic workflow has 1 pods: 0 running, 0 starting, 0 terminating and 1 in error
┃  |__ Job agentic-workflow-z5a0dd39e
┃     |__ ⚠️ Error creating: pods "agentic-workflow-z5a0dd39e-" is forbidden
┃  |__ Pod agentic-workflow-pod-1 is FAILING
┃     |__ 💭 Pod have been killed due to lack of/using too much memory resources
┃     |__ 💢 Container agent crashed 5 times. Last terminated with exit code 137 due to OOMKilled using too much memory at 1970-01-01T00:00:00Z
┃     |__ 💭 Exit code 137 means the container was immediately killed by the operating system via SIGKILL signal.
			The most likely cause is your application running out of memory. Look at your metrics and/or try to increase memory
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

        assert_eq!(rendered_report.lines().count(), gold_standard.lines().count());
        for (rendered_line, gold_line) in rendered_report.lines().zip(gold_standard.lines()) {
            assert_eq!(rendered_line.trim_end(), gold_line);
        }
    }
}
