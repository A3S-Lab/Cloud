use crate::modules::workflow::application::WorkflowRunMutationResult;
use crate::modules::workflow::domain::{
    WorkflowRunRecord, WorkflowRunVariable, WorkflowRunVariableInspection, WorkflowStepProjection,
};
use crate::modules::workflow::WorkflowRunOutput;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStepProjectionResponse {
    pub step_id: String,
    pub kind: String,
    pub status: String,
    pub flow_step_id: String,
    pub attempt_generation: u32,
    pub selected_handle: Option<String>,
    pub result: Option<serde_json::Value>,
    pub result_digest: Option<String>,
    pub error: Option<String>,
    pub evidence_references: Vec<String>,
    pub last_flow_sequence: u64,
    pub updated_at: DateTime<Utc>,
}

impl From<WorkflowStepProjection> for WorkflowStepProjectionResponse {
    fn from(value: WorkflowStepProjection) -> Self {
        Self {
            step_id: value.step_id,
            kind: value.kind.as_str().to_owned(),
            status: value.status.as_str().to_owned(),
            flow_step_id: value.flow_step_id,
            attempt_generation: value.attempt_generation,
            selected_handle: value.selected_handle,
            result: value.result,
            result_digest: value.result_digest.map(|digest| digest.to_string()),
            error: value.error,
            evidence_references: value.evidence_references,
            last_flow_sequence: value.last_flow_sequence,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub id: Uuid,
    pub workflow_goal_id: Uuid,
    pub plan_revision_id: Uuid,
    pub plan_digest: String,
    pub operation_id: Uuid,
    pub flow_run_id: String,
    pub flow_runtime_build_id: Option<String>,
    pub execution_input_digest: String,
    pub status: String,
    pub last_flow_sequence: u64,
    pub output_digest: Option<String>,
    pub error: Option<String>,
    pub aggregate_version: u64,
    pub requested_by: Uuid,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub deadline_at: DateTime<Utc>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub cancellation_requested_by: Option<Uuid>,
    pub cancellation_reason: Option<String>,
    pub finished_at: Option<DateTime<Utc>>,
    pub steps: Vec<WorkflowStepProjectionResponse>,
}

impl From<WorkflowRunRecord> for WorkflowRunResponse {
    fn from(value: WorkflowRunRecord) -> Self {
        let deadline_at = value.run.execution_input.deadline_at;
        Self {
            organization_id: value.run.organization_id.as_uuid(),
            project_id: value.run.project_id.as_uuid(),
            id: value.run.id.as_uuid(),
            workflow_goal_id: value.run.workflow_goal_id.as_uuid(),
            plan_revision_id: value.run.plan_revision_id.as_uuid(),
            plan_digest: value.run.plan_digest.to_string(),
            operation_id: value.run.operation_id.as_uuid(),
            flow_run_id: value.run.flow_run_id,
            flow_runtime_build_id: value.run.flow_runtime_build_id,
            execution_input_digest: value.run.execution_input_digest.to_string(),
            status: value.run.status.as_str().to_owned(),
            last_flow_sequence: value.run.last_flow_sequence,
            output_digest: value.run.output_digest.map(|digest| digest.to_string()),
            error: value.run.error,
            aggregate_version: value.run.aggregate_version,
            requested_by: value.run.requested_by.as_uuid(),
            requested_at: value.run.requested_at,
            updated_at: value.run.updated_at,
            started_at: value.run.started_at,
            deadline_at,
            cancellation_requested_at: value.run.cancellation_requested_at,
            cancellation_requested_by: value
                .run
                .cancellation_requested_by
                .map(|principal_id| principal_id.as_uuid()),
            cancellation_reason: value.run.cancellation_reason,
            finished_at: value.run.finished_at,
            steps: value
                .steps
                .into_iter()
                .map(WorkflowStepProjectionResponse::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunMutationResponse {
    pub workflow_run: WorkflowRunResponse,
    pub replayed: bool,
}

impl From<WorkflowRunMutationResult> for WorkflowRunMutationResponse {
    fn from(value: WorkflowRunMutationResult) -> Self {
        Self {
            workflow_run: WorkflowRunResponse::from(value.record),
            replayed: value.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunOutputResponse {
    pub workflow_run_id: Uuid,
    pub output: serde_json::Value,
    pub output_digest: String,
    pub finished_at: DateTime<Utc>,
}

impl From<WorkflowRunOutput> for WorkflowRunOutputResponse {
    fn from(value: WorkflowRunOutput) -> Self {
        Self {
            workflow_run_id: value.workflow_run_id.as_uuid(),
            output: value.output,
            output_digest: value.output_digest.to_string(),
            finished_at: value.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunVariableResponse {
    pub name: String,
    pub scope: String,
    pub value_type: String,
    pub value_schema_digest: String,
    pub storage_class: String,
    pub mutation_mode: String,
    pub required: bool,
    pub source_step_id: Option<String>,
    pub state: String,
    pub redacted: bool,
    pub value: Option<serde_json::Value>,
    pub value_digest: Option<String>,
}

impl From<WorkflowRunVariable> for WorkflowRunVariableResponse {
    fn from(value: WorkflowRunVariable) -> Self {
        Self {
            name: value.name,
            scope: value.scope.as_str().into(),
            value_type: value.value_type.as_str().into(),
            value_schema_digest: value.value_schema_digest.to_string(),
            storage_class: value.storage_class.as_str().into(),
            mutation_mode: value.mutation_mode.as_str().into(),
            required: value.required,
            source_step_id: value.source_step_id,
            state: value.state.as_str().into(),
            redacted: value.redacted,
            value: value.value,
            value_digest: value.value_digest.map(|digest| digest.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunVariableInspectionResponse {
    pub schema: String,
    pub workflow_run_id: Uuid,
    pub plan_revision_id: Uuid,
    pub variable_contract_digest: String,
    pub last_flow_sequence: u64,
    pub observed_at: DateTime<Utc>,
    pub variables: Vec<WorkflowRunVariableResponse>,
}

impl From<WorkflowRunVariableInspection> for WorkflowRunVariableInspectionResponse {
    fn from(value: WorkflowRunVariableInspection) -> Self {
        Self {
            schema: value.schema,
            workflow_run_id: value.workflow_run_id.as_uuid(),
            plan_revision_id: value.plan_revision_id.as_uuid(),
            variable_contract_digest: value.variable_contract_digest.to_string(),
            last_flow_sequence: value.last_flow_sequence,
            observed_at: value.observed_at,
            variables: value
                .variables
                .into_iter()
                .map(WorkflowRunVariableResponse::from)
                .collect(),
        }
    }
}
