use crate::models::TaskSelector;
use crate::utils::Mode;
use lazy_static::lazy_static;
use qovery_engine::events::EngineEvent;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub struct Subject {
    pub(crate) name: String,
}

impl Subject {
    pub fn new(mode: &Mode, task_selector: &TaskSelector) -> Self {
        let suffix = match task_selector {
            TaskSelector::Infrastructure(s) => s,
            TaskSelector::Environment(s) => s,
        };

        let name = match mode {
            Mode::Local => format!("engine.local.{}", suffix),
            Mode::Cloud(organization, cloud_provider, region) => {
                format!("engine.cloud.{}.{}.{}.{}", organization, cloud_provider, region, suffix)
            }
        };
        Subject { name }
    }

    pub fn new_for_engine_event(engine_event: EngineEvent) -> Self {
        let event_details = engine_event.get_details();

        Subject {
            name: format!(
                "engine.logs.{}.{}.{}.{}",
                event_details.organisation_id().to_string().to_lowercase(),
                event_details.provider_kind().to_string().to_lowercase(),
                event_details.region().to_lowercase(),
                event_details.stage().to_string().to_lowercase()
            ),
        }
    }
}

impl Display for Subject {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

lazy_static! {
    pub static ref CORE_TASK_STATUS: Subject = Subject {
        name: "core.task.status".to_string(),
    };
    pub static ref CORE_PING: Subject = Subject {
        name: "core.ping".to_string(),
    };
}
