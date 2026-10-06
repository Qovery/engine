use crate::grpc::engine::StepLabel as GrpcStepLabel;
use crate::grpc::engine::StepRecord as GrpcStepRecord;
use crate::grpc::engine::StepStatus as GrpcStepStatus;
use qovery_engine::metrics_registry::{StepLabel, StepRecord, StepStatus};

impl GrpcStepRecord {
    pub fn from_record(step_record: StepRecord) -> Self {
        let built_image = step_record.built_image.clone();
        GrpcStepRecord {
            id: step_record.id.to_string(),
            step_id: step_record.step_id.to_string(),
            step_label: match step_record.label {
                StepLabel::Service => GrpcStepLabel::Service as i32,
                StepLabel::Environment => GrpcStepLabel::Environment as i32,
            },
            step_name: step_record.step_name.to_string(),
            duration: step_record.duration.map(|d| prost_types::Duration {
                seconds: d.as_secs() as i64,
                nanos: d.subsec_nanos() as i32,
            }),
            status: match step_record.status.as_ref().unwrap_or(&StepStatus::NotSet) {
                StepStatus::Ongoing => GrpcStepStatus::Ongoing as i32,
                StepStatus::Success => GrpcStepStatus::Success as i32,
                StepStatus::Error => GrpcStepStatus::Error as i32,
                StepStatus::Cancel => GrpcStepStatus::Cancel as i32,
                StepStatus::Skip => GrpcStepStatus::Skip as i32,
                StepStatus::NotSet => GrpcStepStatus::NotSet as i32,
            },
            started_at: Some(step_record.started_at.into()),
            image_name: built_image.as_ref().map(|image| image.name.clone()).unwrap_or_default(),
            image_tag: built_image.as_ref().map(|image| image.tag.clone()).unwrap_or_default(),
            tag_variable_names: built_image.map(|image| image.tag_variable_names).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GrpcStepRecord, GrpcStepStatus, StepLabel, StepRecord, StepStatus};
    use qovery_engine::metrics_registry::{BuiltImage, StepName};
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn ongoing_record_conversion_preserves_its_lifecycle_fields() {
        let mut step_record = StepRecord::new(StepName::Build, StepLabel::Service, Uuid::new_v4());
        step_record.duration = Some(Duration::ZERO);
        step_record.status = Some(StepStatus::Ongoing);
        let expected_step_id = step_record.step_id.to_string();
        let expected_started_at = step_record.started_at.into();

        let grpc_record = GrpcStepRecord::from_record(step_record);

        assert_eq!(grpc_record.step_id, expected_step_id);
        assert_eq!(grpc_record.started_at, Some(expected_started_at));
        assert_eq!(grpc_record.status, GrpcStepStatus::Ongoing as i32);
        assert_eq!(grpc_record.duration, Some(prost_types::Duration::default()));
    }

    #[test]
    fn built_image_conversion_carries_name_tag_and_variable_names() {
        let mut step_record = StepRecord::new(StepName::Build, StepLabel::Service, Uuid::new_v4());
        step_record.status = Some(StepStatus::Success);
        step_record.built_image = Some(BuiltImage {
            name: "repo-image".to_string(),
            tag: "abc123".to_string(),
            tag_variable_names: vec!["API_URL".to_string(), "NODE_ENV".to_string()],
        });

        let grpc_record = GrpcStepRecord::from_record(step_record);

        assert_eq!(grpc_record.image_name, "repo-image");
        assert_eq!(grpc_record.image_tag, "abc123");
        assert_eq!(
            grpc_record.tag_variable_names,
            vec!["API_URL".to_string(), "NODE_ENV".to_string()]
        );
    }

    #[test]
    fn record_without_built_image_converts_to_empty_image_fields() {
        let grpc_record =
            GrpcStepRecord::from_record(StepRecord::new(StepName::Deployment, StepLabel::Service, Uuid::new_v4()));

        assert!(grpc_record.image_name.is_empty());
        assert!(grpc_record.image_tag.is_empty());
        assert!(grpc_record.tag_variable_names.is_empty());
    }
}
