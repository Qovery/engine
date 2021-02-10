use crate::models::TaskSelector;
use crate::utils::Mode;
use lazy_static::lazy_static;

#[derive(Debug)]
pub struct Subject {
    pub(crate) name: String,
}

lazy_static! {
    pub static ref CORE_TASK_STATUS: Subject = Subject {
        name: "core.task.status".to_string(),
    };
    pub static ref CORE_PING: Subject = Subject {
        name: "core.ping".to_string(),
    };
}

pub fn get_subject_name(mode: &Mode, task_selector: &TaskSelector) -> Subject {
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
