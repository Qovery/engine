use crate::events::{EngineMsg, EngineMsgPayload};
use crate::msg_publisher::{MsgPublisher, StdMsgPublisher};
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum StepName {
    Total,
    ProvisionBuilder,
    RegistryCreateRepository,
    GitClone,
    BuildQueueing,
    Build,
    MirrorImage,
    DeploymentQueueing,
    Deployment,
    Executing,
}

impl Display for StepName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            StepName::Total => "Total".to_string(),
            StepName::ProvisionBuilder => "ProvisionBuilder".to_string(),
            StepName::RegistryCreateRepository => "RegistryCreateRepository".to_string(),
            StepName::BuildQueueing => "BuildQueueing".to_string(),
            StepName::GitClone => "GitClone".to_string(),
            StepName::Build => "Build".to_string(),
            StepName::MirrorImage => "MirrorImage".to_string(),
            StepName::DeploymentQueueing => "DeploymentQueueing".to_string(),
            StepName::Deployment => "Deployment".to_string(),
            StepName::Executing => "Executing".to_string(),
        };
        write!(f, "{str}")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StepLabel {
    Service,
    Environment,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StepStatus {
    Ongoing,
    Success,
    Error,
    Cancel,
    Skip,
    NotSet,
}

/// Image a Build step produced or reused, reported with the step so builds can be counted per tag.
/// Variable names only: values never leave the engine.
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltImage {
    pub name: String,
    pub tag: String,
    pub tag_variable_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StepRecord {
    pub step_id: Uuid,
    pub step_name: StepName,
    pub label: StepLabel,
    pub id: Uuid,
    pub started_at: SystemTime,
    start_time: Instant,
    pub duration: Option<Duration>,
    pub status: Option<StepStatus>,
    pub built_image: Option<BuiltImage>,
}

#[derive(Clone)]
pub struct StepRecordHandle<'a> {
    id: Uuid,
    name: StepName,
    metrics_registry: &'a dyn MetricsRegistry,
}

pub trait MetricsRegistry: Send + Sync {
    fn start_record(&self, id: Uuid, label: StepLabel, step_name: StepName) -> StepRecordHandle<'_>;
    fn stop_record(&self, id: Uuid, deployment_step: StepName, status: StepStatus);
    fn set_built_image(&self, id: Uuid, deployment_step: StepName, built_image: BuiltImage);
    fn record_is_stopped(&self, id: Uuid, deployment_step: StepName) -> bool;
    fn get_records(&self, service_id: Uuid) -> Vec<StepRecord>;
    fn clear(&self);
    fn clone_dyn(&self) -> Box<dyn MetricsRegistry>;
}

impl Clone for Box<dyn MetricsRegistry> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

impl StepRecord {
    pub fn new(step_name: StepName, label: StepLabel, id: Uuid) -> Self {
        StepRecord {
            step_id: Uuid::new_v4(),
            step_name,
            label,
            id,
            started_at: SystemTime::now(),
            start_time: Instant::now(),
            duration: None,
            status: None,
            built_image: None,
        }
    }
}

impl<'a> StepRecordHandle<'a> {
    pub fn new(id: Uuid, name: StepName, metrics_registry: &'a dyn MetricsRegistry) -> Self {
        StepRecordHandle {
            id,
            name,
            metrics_registry,
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.metrics_registry.record_is_stopped(self.id, self.name.clone())
    }

    pub fn stop(&self, status: StepStatus) {
        self.metrics_registry.stop_record(self.id, self.name.clone(), status);
    }

    /// Attach before `stop`: the stop message is the one that carries it.
    pub fn set_built_image(&self, built_image: BuiltImage) {
        self.metrics_registry
            .set_built_image(self.id, self.name.clone(), built_image);
    }
}

type StepRecordMap = HashMap<StepName, StepRecord>;

struct MetricsRegistryMap {
    map: Mutex<HashMap<Uuid, StepRecordMap>>,
}

impl MetricsRegistryMap {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Clone)]
pub struct StdMetricsRegistry {
    registry: Arc<MetricsRegistryMap>,
    message_publisher: Arc<dyn MsgPublisher>,
}

impl StdMetricsRegistry {
    pub fn new(message_publisher: Box<dyn MsgPublisher>) -> Self {
        StdMetricsRegistry {
            registry: Arc::new(MetricsRegistryMap::new()),
            message_publisher: Arc::from(message_publisher),
        }
    }
}

impl Default for StdMetricsRegistry {
    fn default() -> Self {
        Self::new(Box::<StdMsgPublisher>::default())
    }
}

impl MetricsRegistry for StdMetricsRegistry {
    fn start_record(&self, id: Uuid, label: StepLabel, step_name: StepName) -> StepRecordHandle<'_> {
        debug!("start record deployment step {:#?} for item {}", step_name, id);

        let mut registry = self.registry.map.lock().unwrap();
        let metrics_per_id = registry.entry(id).or_default();

        if metrics_per_id.contains_key(&step_name) {
            error!("key {:#?} already exist", step_name);
        }

        let deployment_step_record = StepRecord::new(step_name.clone(), label, id);
        metrics_per_id.insert(step_name.clone(), deployment_step_record.clone());

        let mut ongoing_step_record = deployment_step_record;
        ongoing_step_record.duration = Some(Duration::ZERO);
        ongoing_step_record.status = Some(StepStatus::Ongoing);
        self.message_publisher
            .send(EngineMsg::new(EngineMsgPayload::Metrics(ongoing_step_record)));

        StepRecordHandle::new(id, step_name, self)
    }

    fn stop_record(&self, id: Uuid, step_name: StepName, status: StepStatus) {
        let mut registry = self.registry.map.lock().expect("Failed to acquire lock");
        let metrics_per_id = registry.entry(id).or_default();

        if let Some(deployment_step_record) = metrics_per_id.get_mut(&step_name) {
            if deployment_step_record.duration.is_some() {
                return;
            }

            debug!("stop record deployment step {:#?} for item {}", step_name, id);
            deployment_step_record.duration = Some(deployment_step_record.start_time.elapsed());
            deployment_step_record.status = Some(status);

            if deployment_step_record.status != Some(StepStatus::NotSet) {
                self.message_publisher
                    .send(EngineMsg::new(EngineMsgPayload::Metrics(deployment_step_record.clone())))
            };
        } else {
            error!(
                "stop record deployment step {:#?} for service {} that has not been started",
                step_name, id
            );
        }
    }

    fn set_built_image(&self, id: Uuid, step_name: StepName, built_image: BuiltImage) {
        let mut registry = self.registry.map.lock().expect("Failed to acquire lock");
        if let Some(deployment_step_record) = registry.entry(id).or_default().get_mut(&step_name)
            && deployment_step_record.duration.is_none()
        {
            deployment_step_record.built_image = Some(built_image);
        }
    }

    fn record_is_stopped(&self, id: Uuid, step_name: StepName) -> bool {
        let mut locked_registry = self.registry.map.lock().unwrap();
        let metrics_per_id = locked_registry.entry(id).or_default();
        if let Some(deployment_step_record) = metrics_per_id.get(&step_name)
            && deployment_step_record.duration.is_some()
        {
            return true;
        }
        false
    }

    fn get_records(&self, id: Uuid) -> Vec<StepRecord> {
        debug!("get step durations for item ${}", id);

        let mut registry = self.registry.map.lock().unwrap();
        let metrics_per_service = registry.entry(id).or_default();
        metrics_per_service
            .values()
            .filter(|record| record.duration.is_some())
            .cloned()
            .collect()
    }

    fn clear(&self) {
        debug!("clear the registry");
        let mut registry = self.registry.map.lock().unwrap();
        registry.clear()
    }

    fn clone_dyn(&self) -> Box<dyn MetricsRegistry> {
        Box::new(self.clone())
    }
}

impl Drop for MetricsRegistryMap {
    fn drop(&mut self) {
        let registry = self.map.lock().unwrap();
        registry.iter().for_each(|(id, step_record_map)| {
            step_record_map.values().for_each(|step_record| {
                if step_record.status.is_none() || step_record.status == Some(StepStatus::NotSet) {
                    warn!(
                        "step record {:?} for service {} has not been stopped correctly",
                        step_record.step_name, *id
                    );
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::events::EngineMsgPayload;
    use crate::metrics_registry::{BuiltImage, MetricsRegistry, StdMetricsRegistry, StepLabel, StepName, StepStatus};
    use crate::msg_publisher::StdMsgPublisher;
    use std::time::Duration;
    use tokio::sync::mpsc::unbounded_channel;
    use uuid::Uuid;

    #[test]
    fn test_get_records_when_registry_is_empty() {
        let service_id = Uuid::new_v4();
        let metrics_registry = StdMetricsRegistry::new(Box::new(StdMsgPublisher::new()));

        let record_infos = metrics_registry.get_records(service_id);
        assert_eq!(record_infos, vec![]);
    }

    #[test]
    fn test_get_records_when_registry_is_not_empty() {
        let service_id = Uuid::new_v4();
        let step_name = StepName::Deployment;
        let step_label = StepLabel::Service;
        let step_status = StepStatus::Success;
        let metrics_registry = StdMetricsRegistry::new(Box::new(StdMsgPublisher::new()));

        {
            // to trigger the record drop
            metrics_registry.start_record(service_id, step_label, step_name.clone());
            metrics_registry.stop_record(service_id, step_name.clone(), step_status.clone());
        }

        let records = metrics_registry.get_records(service_id);
        assert_eq!(records.len(), 1);
        assert_eq!(records.first().unwrap().step_name, step_name);
        assert_eq!(records.first().unwrap().id, service_id);
        assert!(records.first().unwrap().duration.is_some());
        assert_eq!(records.first().unwrap().status, Some(step_status));
    }

    #[test]
    fn test_get_records_when_record_is_stopped() {
        let service_id = Uuid::new_v4();
        let step_name = StepName::Deployment;
        let step_label = StepLabel::Service;
        let step_status = StepStatus::Success;
        let metrics_registry = StdMetricsRegistry::new(Box::new(StdMsgPublisher::new()));

        {
            // to trigger the record drop
            let record = metrics_registry.start_record(service_id, step_label, step_name.clone());
            record.stop(step_status.clone());
        }

        let records = metrics_registry.get_records(service_id);
        assert_eq!(records.len(), 1);
        assert_eq!(records.first().unwrap().step_name, step_name);
        assert_eq!(records.first().unwrap().id, service_id);
        assert!(records.first().unwrap().duration.is_some());
        assert_eq!(records.first().unwrap().status, Some(step_status));
    }

    #[test]
    fn test_start_and_stop_publish_the_same_step_lifecycle() {
        let service_id = Uuid::new_v4();
        let (publisher, mut messages) = unbounded_channel();
        let metrics_registry = StdMetricsRegistry::new(Box::new(publisher));

        let record = metrics_registry.start_record(service_id, StepLabel::Service, StepName::Build);

        let started_message = messages.try_recv().unwrap();
        let EngineMsgPayload::Metrics(started_record) = started_message.payload;
        assert_eq!(started_record.status, Some(StepStatus::Ongoing));
        assert_eq!(started_record.duration, Some(Duration::ZERO));

        record.stop(StepStatus::Success);

        let completed_message = messages.try_recv().unwrap();
        let EngineMsgPayload::Metrics(completed_record) = completed_message.payload;
        assert_eq!(completed_record.status, Some(StepStatus::Success));
        assert_eq!(completed_record.step_id, started_record.step_id);
        assert_eq!(completed_record.started_at, started_record.started_at);
        assert!(completed_record.duration.is_some());
        assert!(messages.try_recv().is_err());
    }

    fn built_image() -> BuiltImage {
        BuiltImage {
            name: "repo-image".to_string(),
            tag: "abc123".to_string(),
            tag_variable_names: vec!["NODE_ENV".to_string()],
        }
    }

    #[test]
    fn built_image_is_published_with_the_stop_message_only() {
        let (publisher, mut messages) = unbounded_channel();
        let metrics_registry = StdMetricsRegistry::new(Box::new(publisher));

        let record = metrics_registry.start_record(Uuid::new_v4(), StepLabel::Service, StepName::Build);
        let EngineMsgPayload::Metrics(started_record) = messages.try_recv().unwrap().payload;
        record.set_built_image(built_image());
        record.stop(StepStatus::Skip);
        let EngineMsgPayload::Metrics(stopped_record) = messages.try_recv().unwrap().payload;

        assert_eq!(started_record.built_image, None);
        assert_eq!(stopped_record.built_image, Some(built_image()));
    }

    #[test]
    fn built_image_set_after_the_stop_is_ignored() {
        let service_id = Uuid::new_v4();
        let metrics_registry = StdMetricsRegistry::new(Box::new(StdMsgPublisher::new()));

        let record = metrics_registry.start_record(service_id, StepLabel::Service, StepName::Build);
        record.stop(StepStatus::Success);
        record.set_built_image(built_image());

        assert_eq!(metrics_registry.get_records(service_id)[0].built_image, None);
    }
}
