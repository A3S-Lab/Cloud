use super::schema::{WorkflowRuns, WorkflowStepProjections};
use crate::infrastructure::PostgresPersistenceError;
use crate::modules::shared_kernel::domain::{
    OperationId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowGoalId, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    WorkflowRun, WorkflowRunInput, WorkflowRunStatus, WorkflowStepKind, WorkflowStepProjection,
    WorkflowStepProjectionStatus,
};
use a3s_orm::expression::Selection;
use a3s_orm::{DecodeError, Expression, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

pub(super) struct WorkflowRunRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    workflow_goal_id: Uuid,
    plan_revision_id: Uuid,
    plan_digest: String,
    operation_id: Uuid,
    flow_run_id: String,
    flow_runtime_build_id: Option<String>,
    execution_input: String,
    execution_input_digest: String,
    status: String,
    last_flow_sequence: u64,
    output: Option<Value>,
    output_digest: Option<String>,
    error: Option<String>,
    aggregate_version: u64,
    requested_by: Uuid,
    requested_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    cancellation_requested_at: Option<DateTime<Utc>>,
    cancellation_requested_by: Option<Uuid>,
    cancellation_reason: Option<String>,
    finished_at: Option<DateTime<Utc>>,
}

pub(super) struct WorkflowRunSelection;

impl Selection for WorkflowRunSelection {
    type Output = WorkflowRunRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkflowRuns::organization_id().expression(),
            WorkflowRuns::project_id().expression(),
            WorkflowRuns::id().expression(),
            WorkflowRuns::workflow_goal_id().expression(),
            WorkflowRuns::plan_revision_id().expression(),
            WorkflowRuns::plan_digest().expression(),
            WorkflowRuns::operation_id().expression(),
            WorkflowRuns::flow_run_id().expression(),
            WorkflowRuns::flow_runtime_build_id().expression(),
            WorkflowRuns::execution_input().expression(),
            WorkflowRuns::execution_input_digest().expression(),
            WorkflowRuns::status().expression(),
            WorkflowRuns::last_flow_sequence().expression(),
            WorkflowRuns::output().expression(),
            WorkflowRuns::output_digest().expression(),
            WorkflowRuns::error().expression(),
            WorkflowRuns::aggregate_version().expression(),
            WorkflowRuns::requested_by().expression(),
            WorkflowRuns::requested_at().expression(),
            WorkflowRuns::updated_at().expression(),
            WorkflowRuns::started_at().expression(),
            WorkflowRuns::cancellation_requested_at().expression(),
            WorkflowRuns::cancellation_requested_by().expression(),
            WorkflowRuns::cancellation_reason().expression(),
            WorkflowRuns::finished_at().expression(),
        ]
    }
}

impl FromRow for WorkflowRunRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            id: decode(row, 2)?,
            workflow_goal_id: decode(row, 3)?,
            plan_revision_id: decode(row, 4)?,
            plan_digest: decode(row, 5)?,
            operation_id: decode(row, 6)?,
            flow_run_id: decode(row, 7)?,
            flow_runtime_build_id: decode(row, 8)?,
            execution_input: decode(row, 9)?,
            execution_input_digest: decode(row, 10)?,
            status: decode(row, 11)?,
            last_flow_sequence: decode(row, 12)?,
            output: decode(row, 13)?,
            output_digest: decode(row, 14)?,
            error: decode(row, 15)?,
            aggregate_version: decode(row, 16)?,
            requested_by: decode(row, 17)?,
            requested_at: decode(row, 18)?,
            updated_at: decode(row, 19)?,
            started_at: decode(row, 20)?,
            cancellation_requested_at: decode(row, 21)?,
            cancellation_requested_by: decode(row, 22)?,
            cancellation_reason: decode(row, 23)?,
            finished_at: decode(row, 24)?,
        })
    }
}

pub(super) fn decode_run(row: WorkflowRunRow) -> Result<WorkflowRun, PostgresPersistenceError> {
    let execution_input = serde_json::from_str::<WorkflowRunInput>(&row.execution_input)?;
    let canonical = String::from_utf8(
        execution_input
            .canonical_bytes()
            .map_err(PostgresPersistenceError::Invariant)?,
    )
    .map_err(|_| {
        PostgresPersistenceError::Invariant("stored WorkflowRun input was not UTF-8".into())
    })?;
    if canonical != row.execution_input {
        return Err(PostgresPersistenceError::Invariant(
            "stored WorkflowRun input is not canonical".into(),
        ));
    }
    WorkflowRun {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        id: WorkflowRunId::from_uuid(row.id),
        workflow_goal_id: WorkflowGoalId::from_uuid(row.workflow_goal_id),
        plan_revision_id: PlanRevisionId::from_uuid(row.plan_revision_id),
        plan_digest: parse_digest(row.plan_digest, "plan")?,
        operation_id: OperationId::from_uuid(row.operation_id),
        flow_run_id: row.flow_run_id,
        flow_runtime_build_id: row.flow_runtime_build_id,
        execution_input,
        execution_input_digest: parse_digest(row.execution_input_digest, "input")?,
        status: WorkflowRunStatus::parse(&row.status)
            .map_err(PostgresPersistenceError::Invariant)?,
        last_flow_sequence: row.last_flow_sequence,
        output: row.output,
        output_digest: row
            .output_digest
            .map(|value| parse_digest(value, "output"))
            .transpose()?,
        error: row.error,
        aggregate_version: row.aggregate_version,
        requested_by: PrincipalId::from_uuid(row.requested_by),
        requested_at: row.requested_at,
        updated_at: row.updated_at,
        started_at: row.started_at,
        cancellation_requested_at: row.cancellation_requested_at,
        cancellation_requested_by: row.cancellation_requested_by.map(PrincipalId::from_uuid),
        cancellation_reason: row.cancellation_reason,
        finished_at: row.finished_at,
    }
    .restore()
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored WorkflowRun is invalid: {error}"))
    })
}

pub(super) struct WorkflowStepRow {
    organization_id: Uuid,
    project_id: Uuid,
    workflow_run_id: Uuid,
    step_id: String,
    kind: String,
    status: String,
    flow_step_id: String,
    attempt_generation: u32,
    selected_handle: Option<String>,
    result: Option<Value>,
    result_digest: Option<String>,
    error: Option<String>,
    evidence_references: Value,
    last_flow_sequence: u64,
    updated_at: DateTime<Utc>,
}

pub(super) struct WorkflowStepSelection;

impl Selection for WorkflowStepSelection {
    type Output = WorkflowStepRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkflowStepProjections::organization_id().expression(),
            WorkflowStepProjections::project_id().expression(),
            WorkflowStepProjections::workflow_run_id().expression(),
            WorkflowStepProjections::step_id().expression(),
            WorkflowStepProjections::kind().expression(),
            WorkflowStepProjections::status().expression(),
            WorkflowStepProjections::flow_step_id().expression(),
            WorkflowStepProjections::attempt_generation().expression(),
            WorkflowStepProjections::selected_handle().expression(),
            WorkflowStepProjections::result().expression(),
            WorkflowStepProjections::result_digest().expression(),
            WorkflowStepProjections::error().expression(),
            WorkflowStepProjections::evidence_references().expression(),
            WorkflowStepProjections::last_flow_sequence().expression(),
            WorkflowStepProjections::updated_at().expression(),
        ]
    }
}

impl FromRow for WorkflowStepRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            workflow_run_id: decode(row, 2)?,
            step_id: decode(row, 3)?,
            kind: decode(row, 4)?,
            status: decode(row, 5)?,
            flow_step_id: decode(row, 6)?,
            attempt_generation: decode(row, 7)?,
            selected_handle: decode(row, 8)?,
            result: decode(row, 9)?,
            result_digest: decode(row, 10)?,
            error: decode(row, 11)?,
            evidence_references: decode(row, 12)?,
            last_flow_sequence: decode(row, 13)?,
            updated_at: decode(row, 14)?,
        })
    }
}

pub(super) fn decode_step(
    row: WorkflowStepRow,
) -> Result<WorkflowStepProjection, PostgresPersistenceError> {
    let evidence_references = serde_json::from_value::<Vec<String>>(row.evidence_references)?;
    WorkflowStepProjection {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        workflow_run_id: WorkflowRunId::from_uuid(row.workflow_run_id),
        step_id: row.step_id,
        kind: WorkflowStepKind::parse(&row.kind).map_err(PostgresPersistenceError::Invariant)?,
        status: WorkflowStepProjectionStatus::parse(&row.status)
            .map_err(PostgresPersistenceError::Invariant)?,
        flow_step_id: row.flow_step_id,
        attempt_generation: row.attempt_generation,
        selected_handle: row.selected_handle,
        result: row.result,
        result_digest: row
            .result_digest
            .map(|value| parse_digest(value, "step result"))
            .transpose()?,
        error: row.error,
        evidence_references,
        last_flow_sequence: row.last_flow_sequence,
        updated_at: row.updated_at,
    }
    .restore()
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored Workflow step projection is invalid: {error}"
        ))
    })
}

fn parse_digest(value: String, label: &str) -> Result<Sha256Digest, PostgresPersistenceError> {
    Sha256Digest::parse(value).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored WorkflowRun {label} digest is invalid: {error}"
        ))
    })
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
