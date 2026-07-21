use crate::environment::models;
use crate::environment::models::agentic_workflow::{AgenticWorkflowError, AgenticWorkflowService};
use crate::infrastructure::models::cloud_provider::service::Action as DomainAction;
use crate::io_models::context::Context;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stub `AgenticWorkflow` service wire payload. The corresponding domain model always renders
/// a single Kubernetes `Job` running `busybox` and printing "hello world", identically for
/// every cloud provider, so there is no `action` field on the wire: the conversion to the
/// domain model always hardcodes `Action::Create`.
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct AgenticWorkflow {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
}

impl AgenticWorkflow {
    pub fn to_agentic_workflow_domain(
        self,
        context: &Context,
    ) -> Result<Box<dyn AgenticWorkflowService>, AgenticWorkflowError> {
        let service = models::agentic_workflow::AgenticWorkflow::new(
            context,
            self.long_id,
            self.name,
            self.kube_name,
            // Hardcoded on purpose: the stub only supports full-environment delete (which calls
            // `on_delete` directly), not per-service removal during an environment update. If an
            // AgenticWorkflow is ever removed individually rather than the whole environment, this
            // hardcoded `Create` means the Job would be left orphaned instead of cleaned up.
            DomainAction::Create,
            |transmitter| context.get_event_details(transmitter),
        )?;

        Ok(Box::new(service))
    }
}
