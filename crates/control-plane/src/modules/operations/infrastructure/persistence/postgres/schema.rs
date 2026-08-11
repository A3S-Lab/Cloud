use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

orm_table! {
    pub(super) struct OperationRequests => "operation_requests" {
        operation_id: Uuid => "operation_id",
        organization_id: Uuid => "organization_id",
        subject_kind: String => "subject_kind",
        subject_id: Uuid => "subject_id",
        workflow_name: String => "workflow_name",
        workflow_version: String => "workflow_version",
        input: Value => "input",
        requested_at: DateTime<Utc> => "requested_at",
    }
}

orm_table! {
    pub(super) struct OperationProjections => "operation_projections" {
        operation_id: Uuid => "operation_id",
        status: String => "status",
        last_sequence: u64 => "last_sequence",
        output: Option<Value> => "output",
        error: Option<String> => "error",
        updated_at: DateTime<Utc> => "updated_at",
    }
}
