use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{OperationId, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRequest {
    pub id: OperationId,
    pub organization_id: OrganizationId,
    pub subject: OperationSubject,
    pub workflow: WorkflowIdentity,
    pub input: serde_json::Value,
    pub requested_at: DateTime<Utc>,
}

impl OperationRequest {
    pub fn new(
        id: OperationId,
        organization_id: OrganizationId,
        subject: OperationSubject,
        workflow: WorkflowIdentity,
        input: serde_json::Value,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            organization_id,
            subject,
            workflow,
            input,
            requested_at,
        }
    }

    pub fn has_same_definition(&self, other: &Self) -> bool {
        self.id == other.id
            && self.organization_id == other.organization_id
            && self.subject == other.subject
            && self.workflow == other.workflow
            && self.input == other.input
    }
}
