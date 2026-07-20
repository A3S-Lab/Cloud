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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_source_revision_id: Option<Uuid>,
}

impl From<OperationRecord> for OperationListItemResponse {
    fn from(record: OperationRecord) -> Self {
        let rollback_source_revision_id = record
            .request
            .input
            .get("rollbackSourceRevisionId")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok());
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
            rollback_source_revision_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::operations::domain::entities::{OperationRecord, OperationRequest};
    use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
    use crate::modules::shared_kernel::domain::{OperationId, OrganizationId};
    use chrono::Utc;

    #[test]
    fn exposes_only_explicit_rollback_lineage() {
        let source_revision_id = Uuid::now_v7();
        let rollback = response(serde_json::json!({
            "rollbackSourceRevisionId": source_revision_id,
        }));
        assert_eq!(
            serde_json::to_value(rollback).expect("serialize rollback operation")
                ["rollbackSourceRevisionId"],
            source_revision_id.to_string()
        );

        let ordinary = serde_json::to_value(response(serde_json::json!({})))
            .expect("serialize ordinary operation");
        assert!(ordinary
            .as_object()
            .is_some_and(|value| !value.contains_key("rollbackSourceRevisionId")));
    }

    fn response(input: serde_json::Value) -> OperationListItemResponse {
        OperationListItemResponse::from(OperationRecord {
            request: OperationRequest::new(
                OperationId::new(),
                OrganizationId::new(),
                OperationSubject::new("deployment", Uuid::now_v7()).expect("operation subject"),
                WorkflowIdentity::new("cloud.deployment", "2").expect("workflow identity"),
                input,
                Utc::now(),
            ),
            projection: None,
        })
    }
}
