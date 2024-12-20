use crate::io_models::annotations_group::{AnnotationsGroup, AnnotationsGroupScope};
use serde_derive::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Debug, Clone)]
pub(crate) struct AnnotationsGroupTeraContext {
    pub(crate) stateful_set: BTreeMap<String, String>,
    pub(crate) deployment: BTreeMap<String, String>,
    pub(crate) service: BTreeMap<String, String>,
    pub(crate) pods: BTreeMap<String, String>,
    pub(crate) secrets: BTreeMap<String, String>,
    pub(crate) hpa: BTreeMap<String, String>,
    pub(crate) ingress: BTreeMap<String, String>,
    pub(crate) job: BTreeMap<String, String>,
    pub(crate) cronjob: BTreeMap<String, String>,
}

impl AnnotationsGroupTeraContext {
    pub fn new(annotations_groups: Vec<AnnotationsGroup>) -> Self {
        Self {
            stateful_set: get_annotations(&annotations_groups, AnnotationsGroupScope::StatefulSets),
            deployment: get_annotations(&annotations_groups, AnnotationsGroupScope::Deployments),
            service: get_annotations(&annotations_groups, AnnotationsGroupScope::Services),
            pods: get_annotations(&annotations_groups, AnnotationsGroupScope::Pods),
            secrets: get_annotations(&annotations_groups, AnnotationsGroupScope::Secrets),
            hpa: get_annotations(&annotations_groups, AnnotationsGroupScope::Hpa),
            ingress: get_annotations(&annotations_groups, AnnotationsGroupScope::Ingress),
            job: get_annotations(&annotations_groups, AnnotationsGroupScope::Jobs),
            cronjob: get_annotations(&annotations_groups, AnnotationsGroupScope::CronJobs),
        }
    }
}

fn get_annotations(annotations_groups: &[AnnotationsGroup], scope: AnnotationsGroupScope) -> BTreeMap<String, String> {
    annotations_groups
        .iter()
        .filter(|annotations_group| annotations_group.scopes.contains(&scope))
        .flat_map(|annotations_group| annotations_group.annotations.clone())
        .map(|annotation| (annotation.key, annotation.value))
        .collect()
}
