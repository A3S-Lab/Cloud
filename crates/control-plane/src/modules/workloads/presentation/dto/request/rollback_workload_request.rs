use crate::modules::shared_kernel::domain::WorkloadRevisionId;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RollbackWorkloadRequest {
    pub revision_id: Uuid,
}

impl RollbackWorkloadRequest {
    pub fn source_revision_id(&self) -> WorkloadRevisionId {
        WorkloadRevisionId::from_uuid(self.revision_id)
    }
}
