use crate::models::TaskSelector;
use crate::utils::Mode;
use lazy_static::lazy_static;
use qovery_engine::deployment_task::Task;
use qovery_engine::events::EngineEvent;
use regex::Regex;
use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub struct SubjectInfo {
    pub(crate) execution_id: Option<String>,
    pub(crate) cluster_id: Option<String>,
    pub(crate) organization_id: Option<String>,
    pub(crate) cloud_provider: Option<String>,
    pub(crate) region: Option<String>,
    pub(crate) suffix: Option<String>,
}

impl SubjectInfo {
    pub fn try_parse(subject: String) -> Option<Self> {
        // Trying from legacy nats log subject
        // Format => "engine.cloud.organization_id.cloud_provider.region.suffix"
        if let Ok(legacy_nats_subject_re) = Regex::new(
            r"^engine.cloud.(?P<organization_id>[a-zA-Z\d\-]*).(?P<cloud_provider>[a-zA-Z\d\-]*).(?P<region>[a-zA-Z\d\-]*).(?P<suffix>[a-zA-Z\d\-]*)$",
        ) {
            if let Some(cap) = legacy_nats_subject_re.captures(subject.as_str()) {
                if let (Some(organization_id), Some(cloud_provider), Some(region), Some(suffix)) = (
                    cap.name("organization_id").map(|e| e.as_str()),
                    cap.name("cloud_provider").map(|e| e.as_str()),
                    cap.name("region").map(|e| e.as_str()),
                    cap.name("suffix").map(|e| e.as_str()),
                ) {
                    return Some(SubjectInfo {
                        execution_id: None,
                        cluster_id: None,
                        organization_id: Some(organization_id.to_string()),
                        cloud_provider: Some(cloud_provider.to_string()),
                        region: Some(region.to_string()),
                        suffix: Some(suffix.to_string()),
                    });
                }
            }
        }

        // Trying from new infra nats log subject
        // Format => "engine.infra.logs.organization_id.cluster_id.execution_id"
        if let Ok(legacy_nats_subject_re) = Regex::new(
            r"^engine.infra.logs.(?P<organization_id>[a-zA-Z\d\-]*).(?P<cluster_id>[a-zA-Z\d\-]*).(?P<execution_id>[a-zA-Z\d\-]*)$",
        ) {
            if let Some(cap) = legacy_nats_subject_re.captures(subject.as_str()) {
                if let (Some(organization_id), Some(cluster_id), Some(execution_id)) = (
                    cap.name("organization_id").map(|e| e.as_str()),
                    cap.name("cluster_id").map(|e| e.as_str()),
                    cap.name("execution_id").map(|e| e.as_str()),
                ) {
                    return Some(SubjectInfo {
                        execution_id: Some(execution_id.to_string()),
                        cluster_id: Some(cluster_id.to_string()),
                        organization_id: Some(organization_id.to_string()),
                        cloud_provider: None,
                        region: None,
                        suffix: None,
                    });
                }
            }
        }

        // Trying from new env nats log subject
        // Format => "engine.env.logs.organization_id.cluster_id.execution_id"
        if let Ok(legacy_nats_subject_re) = Regex::new(
            r"^engine.env.logs.(?P<organization_id>[a-zA-Z\d\-]*).(?P<cluster_id>[a-zA-Z\d\-]*).(?P<execution_id>[a-zA-Z\d\-]*)$",
        ) {
            if let Some(cap) = legacy_nats_subject_re.captures(subject.as_str()) {
                if let (Some(organization_id), Some(cluster_id), Some(execution_id)) = (
                    cap.name("organization_id").map(|e| e.as_str()),
                    cap.name("cluster_id").map(|e| e.as_str()),
                    cap.name("execution_id").map(|e| e.as_str()),
                ) {
                    return Some(SubjectInfo {
                        execution_id: Some(execution_id.to_string()),
                        cluster_id: Some(cluster_id.to_string()),
                        organization_id: Some(organization_id.to_string()),
                        cloud_provider: None,
                        region: None,
                        suffix: None,
                    });
                }
            }
        }

        None
    }
}

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
                "engine.infra.logs.{}.{}.{}",
                event_details.organisation_id(),
                event_details.cluster_id(),
                event_details.execution_id(),
            ),
        }
    }

    pub fn new_for_task_cancel(task: &dyn Task) -> Self {
        Subject {
            name: format!("engine.task.{}.cancel", task.id()),
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

#[cfg(test)]
mod tests {
    use crate::nats::subjects::SubjectInfo;

    struct SubjectTestCase<'a> {
        subject: &'a str,
        expected_subject_info: Option<SubjectInfo>,
    }

    #[test]
    fn test_legacy_nats_subject_parsing() {
        // setup:
        let test_cases = vec![
            SubjectTestCase {
                subject: "engine.cloud.87db1899-b732-4b11-bfb3-8c8b43bbf523.aws.fr-par.infra",
                expected_subject_info: Some(SubjectInfo {
                    execution_id: None,
                    cluster_id: None,
                    organization_id: Some("87db1899-b732-4b11-bfb3-8c8b43bbf523".to_string()),
                    cloud_provider: Some("aws".to_string()),
                    region: Some("fr-par".to_string()),
                    suffix: Some("infra".to_string()),
                }),
            },
            SubjectTestCase {
                subject: "wrong.engine.cloud.87db1899-b732-4b11-bfb3-8c8b43bbf523.aws.fr-par.infra",
                expected_subject_info: None,
            },
            SubjectTestCase {
                subject: "engine.cloud.87db1899-b732-4b11-bfb3-8c8b43bbf523.aws.fr-par.infra.wrong",
                expected_subject_info: None,
            },
        ];

        for tc in test_cases {
            // execute:
            let result = SubjectInfo::try_parse(tc.subject.to_string());

            // validate:
            assert_eq!(tc.expected_subject_info, result);
        }
    }

    #[test]
    fn test_new_nats_infra_subject_parsing() {
        // setup:
        let test_cases = vec![
            SubjectTestCase {
                subject: "engine.infra.logs.87db1899-b732-4b11-bfb3-8c8b43bbf523.87db1899-b732-4b11-bfb3-8c8b43bbf524.87db1899-b732-4b11-bfb3-8c8b43bbf525",
                expected_subject_info: Some(SubjectInfo {
                    execution_id: Some("87db1899-b732-4b11-bfb3-8c8b43bbf525".to_string()),
                    cluster_id: Some("87db1899-b732-4b11-bfb3-8c8b43bbf524".to_string()),
                    organization_id: Some("87db1899-b732-4b11-bfb3-8c8b43bbf523".to_string()),
                    cloud_provider: None,
                    region: None,
                    suffix: None,
                }),
            },
            SubjectTestCase {
                subject: "wrong.engine.infra.logs.87db1899-b732-4b11-bfb3-8c8b43bbf523.87db1899-b732-4b11-bfb3-8c8b43bbf524.87db1899-b732-4b11-bfb3-8c8b43bbf525",
                expected_subject_info: None,
            },
            SubjectTestCase {
                subject: "engine.infra.logs.87db1899-b732-4b11-bfb3-8c8b43bbf523.87db1899-b732-4b11-bfb3-8c8b43bbf524.87db1899-b732-4b11-bfb3-8c8b43bbf525.wrong",
                expected_subject_info: None,
            },
        ];

        for tc in test_cases {
            // execute:
            let result = SubjectInfo::try_parse(tc.subject.to_string());

            // validate:
            assert_eq!(tc.expected_subject_info, result);
        }
    }

    #[test]
    fn test_new_nats_env_subject_parsing() {
        // setup:
        let test_cases = vec![
            SubjectTestCase {
                subject: "engine.env.logs.87db1899-b732-4b11-bfb3-8c8b43bbf523.87db1899-b732-4b11-bfb3-8c8b43bbf524.87db1899-b732-4b11-bfb3-8c8b43bbf525",
                expected_subject_info: Some(SubjectInfo {
                    execution_id: Some("87db1899-b732-4b11-bfb3-8c8b43bbf525".to_string()),
                    cluster_id: Some("87db1899-b732-4b11-bfb3-8c8b43bbf524".to_string()),
                    organization_id: Some("87db1899-b732-4b11-bfb3-8c8b43bbf523".to_string()),
                    cloud_provider: None,
                    region: None,
                    suffix: None,
                }),
            },
            SubjectTestCase {
                subject: "wrong.engine.env.logs.87db1899-b732-4b11-bfb3-8c8b43bbf523.87db1899-b732-4b11-bfb3-8c8b43bbf524.87db1899-b732-4b11-bfb3-8c8b43bbf525",
                expected_subject_info: None,
            },
            SubjectTestCase {
                subject: "engine.env.logs.87db1899-b732-4b11-bfb3-8c8b43bbf523.87db1899-b732-4b11-bfb3-8c8b43bbf524.87db1899-b732-4b11-bfb3-8c8b43bbf525.wrong",
                expected_subject_info: None,
            },
        ];

        for tc in test_cases {
            // execute:
            let result = SubjectInfo::try_parse(tc.subject.to_string());

            // validate:
            assert_eq!(tc.expected_subject_info, result);
        }
    }
}
