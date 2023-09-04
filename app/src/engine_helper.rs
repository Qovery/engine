use crate::grpc::engine::StepLabel as GrpcStepLabel;
use crate::grpc::engine::StepRecord as GrpcStepRecord;
use crate::grpc::engine::StepStatus as GrpcStepStatus;
use qovery_engine::metrics_registry::{StepLabel, StepRecord, StepStatus};

impl GrpcStepRecord {
    pub fn from_record(step_record: StepRecord) -> Self {
        GrpcStepRecord {
            id: step_record.id.to_string(),
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
                StepStatus::Success => GrpcStepStatus::Success as i32,
                StepStatus::Error => GrpcStepStatus::Error as i32,
                StepStatus::Cancel => GrpcStepStatus::Cancel as i32,
                StepStatus::Skip => GrpcStepStatus::Skip as i32,
                StepStatus::NotSet => GrpcStepStatus::NotSet as i32,
            },
        }
    }
}
