use crate::deployment_report::job::reporter::{JobDeploymentReport, JobType};
use crate::deployment_report::utils::{
    get_tera_instance, to_job_render_context, to_pods_render_context_by_version, JobRenderContext, PodsRenderContext,
};
use crate::utilities::to_short_id;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct JobDeploymentRenderContext {
    pub name: String,
    pub job_type: String,
    pub tag: String,
    pub nb_pods: usize,
    pub job: Option<JobRenderContext>,
    pub pods_current_version: PodsRenderContext,
    pub pods_old_version: PodsRenderContext,
}

const REPORT_TEMPLATE: &str = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ {{ job_type | capitalize }} at tag {{ tag }} execution is in progress ⏳, below the current status:
┃
┃ 🛰 {{ job_type | capitalize }} at old version has {{ pods_old_version.nb_pods }} pods: {{ pods_old_version.pods_running | length }} running, {{ pods_old_version.pods_starting | length }} starting, {{ pods_old_version.pods_terminating | length }} terminating and {{ pods_old_version.pods_failing | length }} in error
┃ 🛰 {{ job_type | capitalize }} at new tag {{ tag }} has {{ pods_current_version.nb_pods }} pods: {{ pods_current_version.pods_running | length }} running, {{ pods_current_version.pods_starting | length }} starting, {{ pods_current_version.pods_terminating | length }} terminating and {{ pods_current_version.pods_failing | length }} in error
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
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

pub(super) fn render_job_deployment_report(
    job_type: &JobType,
    service_tag: &str,
    deployment_info: &JobDeploymentReport,
) -> Result<String, tera::Error> {
    let (pods_current_version, pods_old_version): (PodsRenderContext, PodsRenderContext) =
        to_pods_render_context_by_version(&deployment_info.pods, &deployment_info.events, service_tag);
    let job_ctx = deployment_info
        .job
        .as_ref()
        .map(|job| to_job_render_context(job, &deployment_info.events));

    let render_ctx = JobDeploymentRenderContext {
        name: to_short_id(&deployment_info.id),
        job_type: job_type.to_string(),
        tag: service_tag.to_string(),
        nb_pods: deployment_info.pods.len(),
        job: job_ctx,
        pods_current_version,
        pods_old_version,
    };
    let ctx = tera::Context::from_serialize(render_ctx)?;
    get_tera_instance().render_str(REPORT_TEMPLATE, &ctx)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cloud_provider::service::Action;
    use crate::deployment_report::utils::{
        exit_code_to_msg, fmt_event_type, DeploymentState, PodRenderContext, QContainerState, QContainerStateTerminated,
    };
    use crate::utilities::to_short_id;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1;
    use maplit::btreemap;
    use tera::Tera;
    use uuid::Uuid;

    #[test]
    fn test_application_rendering() {
        let app_id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let render_ctx = JobDeploymentRenderContext {
            name: to_short_id(&app_id),
            job_type: JobType::Job(Action::Create).to_string(),
            tag: "public.ecr.aws/r3m4q3r9/pub-mirror-debian:11.6".to_string(),
            nb_pods: 1,
            job: None,
            pods_old_version: PodsRenderContext {
                nb_pods: 0,
                pods_running: vec![],
                pods_starting: vec![],
                pods_failing: vec![],
                pods_terminating: vec![],
            },
            pods_current_version: PodsRenderContext {
                nb_pods: 1,
                pods_failing: vec![PodRenderContext {
                    name: "app-pod-1".to_string(),
                    state: DeploymentState::Failing,
                    message: Some("Pod have been killed due to lack of/using too much memory resources".to_string()),
                    container_states: btreemap! {
                            "app-container-1".to_string() => QContainerState {
                            restart_count: 5u32,
                            last_state: QContainerStateTerminated {
                                    exit_code: 132,
                                    exit_code_msg: exit_code_to_msg(137),
                                    reason:  Some("OOMKilled".to_string()),
                                    message: Some("using too much memory".to_string()),
                                    finished_at: Some(v1::Time(chrono::DateTime::default())),
                            }
                        },
                    },
                    events: vec![],
                    service_version: Some("debian:bookworm".to_string()),
                }],
                pods_starting: vec![],
                pods_terminating: vec![],
                pods_running: vec![],
            },
        };

        let ctx = tera::Context::from_serialize(render_ctx).unwrap();
        let mut tera = Tera::default();
        tera.register_filter("fmt_event_type", fmt_event_type);

        let rendered_report = tera.render_str(REPORT_TEMPLATE, &ctx).unwrap();
        println!("{rendered_report}");

        let gold_standard = r#"
┏━━ 📝 Deployment Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Job at tag public.ecr.aws/r3m4q3r9/pub-mirror-debian:11.6 execution is in progress ⏳, below the current status:
┃
┃ 🛰 Job at old version has 0 pods: 0 running, 0 starting, 0 terminating and 0 in error
┃ 🛰 Job at new tag public.ecr.aws/r3m4q3r9/pub-mirror-debian:11.6 has 1 pods: 0 running, 0 starting, 0 terminating and 1 in error
┃  |__ Pod app-pod-1 is FAILING
┃     |__ 💭 Pod have been killed due to lack of/using too much memory resources
┃     |__ 💢 Container app-container-1 crashed 5 times. Last terminated with exit code 132 due to OOMKilled using too much memory at 1970-01-01T00:00:00Z
┃     |__ 💭 Exit code 132 means the container was immediately terminated by the operating system via SIGKILL signal
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

        for (rendered_line, gold_line) in rendered_report.lines().zip(gold_standard.lines()) {
            assert_eq!(rendered_line.trim_end(), gold_line);
        }
    }
}
