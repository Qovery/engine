use crate::deployment_report::job::reporter::{JobDeploymentReport, JobType};
use crate::deployment_report::utils::{
    get_tera_instance, to_job_render_context, to_pods_render_context, JobRenderContext, PodRenderContext,
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
    pub pods_failing: Vec<PodRenderContext>,
    pub pods_starting: Vec<PodRenderContext>,
    pub pods_terminating: Vec<PodRenderContext>,
    pub pods_running: Vec<PodRenderContext>,
}

const REPORT_TEMPLATE: &str = r#"
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ {{ job_type | capitalize }} at tag {{ tag }} execution is in progress ⏳, below the current status:
┃
{% set all_pods = pods_failing | concat(with=pods_starting) | concat(with=pods_running) -%}
┃ 🛰 {{ job_type | capitalize }} has {{ nb_pods }} pods. {{ pods_starting | length }} starting, {{ pods_terminating | length }} terminating and {{ pods_failing | length }} in error
{%- for pod in all_pods %}
┃  |__ Pod {{ pod.name }} is {{ pod.state | upper }} {{ pod.message }}{%- if pod.restart_count > 0 %}
┃     |__ 💢 Pod crashed {{ pod.restart_count }} times
{%- endif -%}
{%- for event in pod.events %}
┃     |__ {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor -%}
{%- endfor %}
┃
┃ ⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

pub(super) fn render_job_deployment_report(
    job_type: &JobType,
    service_tag: &str,
    deployment_info: &JobDeploymentReport,
) -> Result<String, tera::Error> {
    let (pods_starting, pods_terminating, pods_failing, pods_running) =
        to_pods_render_context(&deployment_info.pods, &deployment_info.events);

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
        pods_failing,
        pods_starting,
        pods_terminating,
        pods_running,
    };
    let ctx = tera::Context::from_serialize(render_ctx)?;
    get_tera_instance().render_str(REPORT_TEMPLATE, &ctx)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cloud_provider::service::Action;
    use crate::deployment_report::utils::{fmt_event_type, DeploymentState, PodRenderContext};
    use crate::utilities::to_short_id;
    use tera::Tera;
    use uuid::Uuid;

    #[test]
    fn test_application_rendering() {
        let app_id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let render_ctx = JobDeploymentRenderContext {
            name: to_short_id(&app_id),
            job_type: JobType::Job(Action::Create).to_string(),
            tag: "docker.io/debian:bullseye".to_string(),
            nb_pods: 1,
            job: None,
            pods_failing: vec![PodRenderContext {
                name: "app-pod-1".to_string(),
                state: DeploymentState::Failing,
                message: Some("pod have been killed due to lack of/using too much memory resources".to_string()),
                restart_count: 5,
                events: vec![],
            }],
            pods_starting: vec![],
            pods_terminating: vec![],
            pods_running: vec![],
        };

        let ctx = tera::Context::from_serialize(render_ctx).unwrap();
        let mut tera = Tera::default();
        tera.register_filter("fmt_event_type", fmt_event_type);

        let rendered_report = tera.render_str(REPORT_TEMPLATE, &ctx).unwrap();
        println!("{rendered_report}");

        let gold_standard = r#"
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃ Job at tag docker.io/debian:bullseye execution is in progress ⏳, below the current status:
┃
┃ 🛰 Job has 1 pods. 0 starting, 0 terminating and 1 in error
┃  |__ Pod app-pod-1 is FAILING pod have been killed due to lack of/using too much memory resources
┃     |__ 💢 Pod crashed 5 times
┃
┃ ⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

        for (rendered_line, gold_line) in rendered_report.lines().zip(gold_standard.lines()) {
            assert_eq!(rendered_line.trim_end(), gold_line);
        }
    }
}
