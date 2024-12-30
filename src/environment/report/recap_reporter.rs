use crate::environment::report::utils::{get_tera_instance, EventRenderContext};
use itertools::Itertools;
use k8s_openapi::api::core::v1::Event;
use serde_derive::Serialize;
use std::collections::HashMap;
use std::time::Instant;

pub struct RecapReporterDeploymentState {
    pub report: String,
    pub timestamp: Instant,
    pub all_warning_events: Vec<Event>,
}

#[derive(Debug, Serialize)]
pub struct RecapRenderContext {
    pub warning_events: Vec<EventRenderContext>,
}

const RECAP_TEMPLATE: &str = r#"
┏━━ 📝 Recap Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
{%- for event in warning_events %}
┃   {{ event.type_ | fmt_event_type }} {{ event.message }}
{%- endfor %}
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;

pub(crate) fn render_recap_events(warning_events: &[Event]) -> Result<String, tera::Error> {
    // aggregate messages to have the number of occurrences
    let event_messages_by_occurrences: HashMap<&str, u16> = warning_events
        .iter()
        .filter_map(|event| event.message.as_ref())
        .fold(HashMap::new(), |mut event_messages_by_occurrences, message| {
            event_messages_by_occurrences
                .entry(message.as_ref())
                .and_modify(|occurrence| *occurrence += 1)
                .or_insert(1);
            event_messages_by_occurrences
        });

    // create manually the event render context
    let warning_events_context = event_messages_by_occurrences
        .iter()
        .sorted_by(|a, b| Ord::cmp(&b.1, &a.1))
        .map(|(k, v)| {
            let message = if *v > 1 {
                format!("{} (x{})", k, v)
            } else {
                k.to_string()
            };
            EventRenderContext {
                message,
                type_: "Warning".to_string(),
            }
        })
        .collect::<Vec<EventRenderContext>>();

    let render_ctx = RecapRenderContext {
        warning_events: warning_events_context,
    };

    let ctx = tera::Context::from_serialize(render_ctx)?;
    get_tera_instance().render_str(RECAP_TEMPLATE, &ctx)
}
#[cfg(test)]
mod test {
    use crate::environment::report::recap_reporter::render_recap_events;
    use k8s_openapi::api::core::v1::Event;

    #[test]
    fn test_recap_rendering() {
        // given
        let event_mock_1 = Event {
            message: Some("Readiness probe failure".to_string()),
            type_: Some("Warning".to_string()),
            ..Default::default()
        };
        let event_mock_2 = Event {
            message: Some("Liveness probe failure".to_string()),
            type_: Some("Warning".to_string()),
            ..Default::default()
        };
        let event_mock_3 = Event {
            message: Some("Readiness probe failure".to_string()),
            type_: Some("Warning".to_string()),
            ..Default::default()
        };
        let event_mocks = vec![event_mock_1, event_mock_2, event_mock_3];

        // when
        let rendered_report = render_recap_events(&event_mocks).unwrap();

        // then
        let expected = r#"
┏━━ 📝 Recap Status Report ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
┃   ⚠️ Readiness probe failure (x2)
┃   ⚠️ Liveness probe failure
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#;
        println!("{rendered_report}");
        for (rendered_line, gold_line) in rendered_report.lines().zip(expected.lines()) {
            assert_eq!(rendered_line.trim_end(), gold_line);
        }
    }
}
