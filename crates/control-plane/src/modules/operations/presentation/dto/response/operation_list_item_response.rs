use crate::modules::operations::domain::entities::{OperationRecord, OperationStatus};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationListItemResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub subject_kind: String,
    pub subject_id: Uuid,
    pub workflow_name: String,
    pub workflow_version: String,
    pub status: OperationStatus,
    pub last_sequence: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub error: Option<String>,
}

impl From<OperationRecord> for OperationListItemResponse {
    fn from(record: OperationRecord) -> Self {
        let projection = record.projection;
        Self {
            id: record.request.id.as_uuid(),
            organization_id: record.request.organization_id.as_uuid(),
            subject_kind: record.request.subject.kind().to_owned(),
            subject_id: record.request.subject.id(),
            workflow_name: record.request.workflow.name().to_owned(),
            workflow_version: record.request.workflow.version().to_owned(),
            status: projection
                .as_ref()
                .map_or(OperationStatus::Queued, |value| value.status),
            last_sequence: projection.as_ref().map_or(0, |value| value.last_sequence),
            requested_at: record.request.requested_at,
            updated_at: projection
                .as_ref()
                .map_or(record.request.requested_at, |value| value.updated_at),
            error: projection.and_then(|value| value.error),
        }
    }
}
