use crate::io_models::labels_group::LabelsGroup;
use serde_derive::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Debug, Clone)]
pub(super) struct LabelsGroupTeraContext {
    pub(super) common: BTreeMap<String, String>,
    pub(super) propagated_to_cloud_provider: BTreeMap<String, String>,
}

impl LabelsGroupTeraContext {
    pub fn new(labels_groups: Vec<LabelsGroup>) -> Self {
        Self {
            common: labels_groups
                .iter()
                .flat_map(|labels_group| labels_group.labels.clone())
                .map(|label| (label.key, label.value))
                .collect(),
            propagated_to_cloud_provider: labels_groups
                .iter()
                .flat_map(|labels_group| labels_group.labels.clone())
                .filter(|label| label.propagate_to_cloud_provider)
                .map(|label| (label.key, label.value))
                .collect(),
        }
    }
}
