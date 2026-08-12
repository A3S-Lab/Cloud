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
    pub(super) struct WorkflowRuns => "workflow_runs" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        id: Uuid => "id",
        workflow_goal_id: Uuid => "workflow_goal_id",
        plan_revision_id: Uuid => "plan_revision_id",
        plan_digest: String => "plan_digest",
        operation_id: Uuid => "operation_id",
        flow_run_id: String => "flow_run_id",
        flow_runtime_build_id: Option<String> => "flow_runtime_build_id",
        execution_input: String => "execution_input",
        execution_input_digest: String => "execution_input_digest",
        status: String => "status",
        last_flow_sequence: u64 => "last_flow_sequence",
        output: Option<Value> => "output",
        output_digest: Option<String> => "output_digest",
        error: Option<String> => "error",
        aggregate_version: u64 => "aggregate_version",
        requested_by: Uuid => "requested_by",
        requested_at: DateTime<Utc> => "requested_at",
        updated_at: DateTime<Utc> => "updated_at",
        started_at: Option<DateTime<Utc>> => "started_at",
        cancellation_requested_at: Option<DateTime<Utc>> => "cancellation_requested_at",
        cancellation_requested_by: Option<Uuid> => "cancellation_requested_by",
        cancellation_reason: Option<String> => "cancellation_reason",
        finished_at: Option<DateTime<Utc>> => "finished_at",
    }
}

orm_table! {
    pub(super) struct WorkflowStepProjections => "workflow_step_projections" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        workflow_run_id: Uuid => "workflow_run_id",
        step_id: String => "step_id",
        kind: String => "kind",
        status: String => "status",
        flow_step_id: String => "flow_step_id",
        attempt_generation: u32 => "attempt_generation",
        selected_handle: Option<String> => "selected_handle",
        result: Option<Value> => "result",
        result_digest: Option<String> => "result_digest",
        error: Option<String> => "error",
        evidence_references: Value => "evidence_references",
        last_flow_sequence: u64 => "last_flow_sequence",
        updated_at: DateTime<Utc> => "updated_at",
    }
}
