use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OperationId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest, WorkflowGoalId, WorkflowRunId,
};
use crate::modules::workflow::domain::repositories::WorkflowRunWriteReference;
use crate::modules::workflow::domain::{
    CancelWorkflowRunWrite, CreateWorkflowRunWrite, IWorkflowRunRepository, WorkflowRun,
    WorkflowRunInput, WorkflowRunRecord, WorkflowRunStatus, WorkflowStepKind,
    WorkflowStepProjection, WorkflowStepProjectionStatus, WORKFLOW_RUN_FLOW_NAME,
    WORKFLOW_RUN_FLOW_VERSION,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

const SELECT_RUNS: &str = "select r.organization_id, r.project_id, r.id, r.workflow_goal_id, r.plan_revision_id, r.plan_digest, r.operation_id, r.flow_run_id, r.flow_runtime_build_id, r.execution_input, r.execution_input_digest, r.status, r.last_flow_sequence, r.output, r.output_digest, r.error, r.aggregate_version, r.requested_by, r.requested_at, r.updated_at, r.started_at, r.cancellation_requested_at, r.cancellation_reason, r.finished_at from workflow_runs r";
const SELECT_STEPS: &str = "select organization_id, project_id, workflow_run_id, step_id, kind, status, flow_step_id, attempt_generation, selected_handle, result, result_digest, error, evidence_references, last_flow_sequence, updated_at from workflow_step_projections";

#[derive(Clone)]
pub struct PostgresWorkflowRunRepository {
    executor: PostgresExecutor,
}

impl PostgresWorkflowRunRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IWorkflowRunRepository for PostgresWorkflowRunRepository {
    async fn create(
        &self,
        write: CreateWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRunRecord>, RepositoryError> {
        write.record.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = idempotency_replay::<WorkflowRunWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_record(
                                transaction,
                                replay.value.organization_id,
                                replay.value.workflow_run_id,
                                false,
                            )
                            .await?,
                            replayed: true,
                        });
                    }
                    let insertion = async {
                        insert_operation(transaction, &write.record.run).await?;
                        insert_run(transaction, &write.record.run).await?;
                        for step in &write.record.steps {
                            insert_step(transaction, step).await?;
                        }
                        Ok::<(), PostgresPersistenceError>(())
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "WorkflowRun or correlated Operation identity already exists"
                                    .into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_run_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        write.request_id,
                        "workflow.run.requested",
                    )
                    .await?;
                    let reference = WorkflowRunWriteReference {
                        organization_id: write.record.run.organization_id,
                        workflow_run_id: write.record.run.id,
                    };
                    store_idempotency(transaction, &write.idempotency, &reference).await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        workflow_run_id: WorkflowRunId,
    ) -> Result<Option<WorkflowRunRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    match find_run_row(transaction, organization_id, workflow_run_id, false).await?
                    {
                        Some(row) => decode_record(transaction, row).await.map(Some),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<WorkflowRunRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<WorkflowRunRow, _>(
                        transaction,
                        sql_query::<WorkflowRunRow>(SELECT_RUNS)
                            .append(" where r.organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and r.project_id = ")
                            .bind(project_id.as_uuid())
                            .append(" order by r.requested_at desc, r.id desc limit ")
                            .bind(limit),
                    )
                    .await?;
                    let mut records = Vec::with_capacity(rows.len());
                    for row in rows {
                        records.push(decode_record(transaction, row).await?);
                    }
                    Ok(records)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn request_cancellation(
        &self,
        write: CancelWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRunRecord>, RepositoryError> {
        write.record.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = idempotency_replay::<WorkflowRunWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_record(
                                transaction,
                                replay.value.organization_id,
                                replay.value.workflow_run_id,
                                false,
                            )
                            .await?,
                            replayed: true,
                        });
                    }
                    let existing = load_record(
                        transaction,
                        write.record.run.organization_id,
                        write.record.run.id,
                        true,
                    )
                    .await?;
                    validate_cancellation_transition(
                        &existing,
                        &write.record,
                        write.expected_version,
                    )?;
                    persist_run(transaction, &write.record.run, write.expected_version).await?;
                    store_outbox(transaction, &write.event).await?;
                    store_run_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        write.request_id,
                        "workflow.run.cancellation-requested",
                    )
                    .await?;
                    let reference = WorkflowRunWriteReference {
                        organization_id: write.record.run.organization_id,
                        workflow_run_id: write.record.run.id,
                    };
                    store_idempotency(transaction, &write.idempotency, &reference).await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn pending_reconciliation(
        &self,
        limit: usize,
    ) -> Result<Vec<WorkflowRunRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<WorkflowRunRow, _>(
                        transaction,
                        sql_query::<WorkflowRunRow>(SELECT_RUNS)
                            .append(" where r.status not in ('completed', 'failed', 'cancelled', 'timed_out') order by r.requested_at asc, r.id asc limit ")
                            .bind(limit),
                    )
                    .await?;
                    let mut records = Vec::with_capacity(rows.len());
                    for row in rows {
                        records.push(decode_record(transaction, row).await?);
                    }
                    Ok(records)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<WorkflowRunRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(replay) =
                        idempotency_replay::<WorkflowRunWriteReference>(transaction, &idempotency)
                            .await?
                    else {
                        return Ok(None);
                    };
                    load_record(
                        transaction,
                        replay.value.organization_id,
                        replay.value.workflow_run_id,
                        false,
                    )
                    .await
                    .map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn save_projection(
        &self,
        record: WorkflowRunRecord,
        expected_version: u64,
    ) -> Result<WorkflowRunRecord, RepositoryError> {
        record.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let existing =
                        load_record(transaction, record.run.organization_id, record.run.id, true)
                            .await?;
                    validate_projection_transition(&existing, &record, expected_version)?;
                    persist_run(transaction, &record.run, expected_version).await?;
                    for step in &record.steps {
                        persist_step(transaction, step).await?;
                    }
                    load_record(
                        transaction,
                        record.run.organization_id,
                        record.run.id,
                        false,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn insert_operation(
    transaction: &a3s_orm::PostgresTransaction,
    run: &WorkflowRun,
) -> Result<(), PostgresPersistenceError> {
    let operation = OperationRequest::new(
        run.operation_id,
        run.organization_id,
        OperationSubject::new("workflow_run", run.id.as_uuid())
            .map_err(PostgresPersistenceError::Invariant)?,
        WorkflowIdentity::new(WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION)
            .map_err(PostgresPersistenceError::Invariant)?,
        serde_json::to_value(&run.execution_input)?,
        run.requested_at,
    );
    require_one_row(
        "WorkflowRun Operation",
        execute(
            transaction,
            sql_query::<()>("insert into operation_requests (operation_id, organization_id, subject_kind, subject_id, workflow_name, workflow_version, input, requested_at) values (")
                .bind(operation.id.as_uuid())
                .append(", ")
                .bind(operation.organization_id.as_uuid())
                .append(", ")
                .bind(operation.subject.kind())
                .append(", ")
                .bind(operation.subject.id())
                .append(", ")
                .bind(operation.workflow.name())
                .append(", ")
                .bind(operation.workflow.version())
                .append(", ")
                .bind(operation.input)
                .append(", ")
                .bind(operation.requested_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_run(
    transaction: &a3s_orm::PostgresTransaction,
    run: &WorkflowRun,
) -> Result<(), PostgresPersistenceError> {
    let execution_input = String::from_utf8(
        run.execution_input
            .canonical_bytes()
            .map_err(PostgresPersistenceError::Invariant)?,
    )
    .map_err(|_| PostgresPersistenceError::Invariant("WorkflowRun input was not UTF-8".into()))?;
    require_one_row(
        "WorkflowRun",
        execute(
            transaction,
            sql_query::<()>("insert into workflow_runs (organization_id, project_id, id, workflow_goal_id, plan_revision_id, plan_digest, operation_id, flow_run_id, flow_runtime_build_id, execution_input, execution_input_digest, status, last_flow_sequence, output, output_digest, error, aggregate_version, requested_by, requested_at, updated_at, started_at, cancellation_requested_at, cancellation_reason, finished_at) values (")
                .bind(run.organization_id.as_uuid())
                .append(", ")
                .bind(run.project_id.as_uuid())
                .append(", ")
                .bind(run.id.as_uuid())
                .append(", ")
                .bind(run.workflow_goal_id.as_uuid())
                .append(", ")
                .bind(run.plan_revision_id.as_uuid())
                .append(", ")
                .bind(run.plan_digest.as_str())
                .append(", ")
                .bind(run.operation_id.as_uuid())
                .append(", ")
                .bind(run.flow_run_id.as_str())
                .append(", ")
                .bind(run.flow_runtime_build_id.as_deref())
                .append(", ")
                .bind(execution_input)
                .append(", ")
                .bind(run.execution_input_digest.as_str())
                .append(", ")
                .bind(run.status.as_str())
                .append(", ")
                .bind(run.last_flow_sequence)
                .append(", ")
                .bind(run.output.clone())
                .append(", ")
                .bind(run.output_digest.as_ref().map(Sha256Digest::as_str))
                .append(", ")
                .bind(run.error.as_deref())
                .append(", ")
                .bind(run.aggregate_version)
                .append(", ")
                .bind(run.requested_by.as_uuid())
                .append(", ")
                .bind(run.requested_at)
                .append(", ")
                .bind(run.updated_at)
                .append(", ")
                .bind(run.started_at)
                .append(", ")
                .bind(run.cancellation_requested_at)
                .append(", ")
                .bind(run.cancellation_reason.as_deref())
                .append(", ")
                .bind(run.finished_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_step(
    transaction: &a3s_orm::PostgresTransaction,
    step: &WorkflowStepProjection,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "WorkflowStepProjection",
        execute(
            transaction,
            sql_query::<()>("insert into workflow_step_projections (organization_id, project_id, workflow_run_id, step_id, kind, status, flow_step_id, attempt_generation, selected_handle, result, result_digest, error, evidence_references, last_flow_sequence, updated_at) values (")
                .bind(step.organization_id.as_uuid())
                .append(", ")
                .bind(step.project_id.as_uuid())
                .append(", ")
                .bind(step.workflow_run_id.as_uuid())
                .append(", ")
                .bind(step.step_id.as_str())
                .append(", ")
                .bind(step.kind.as_str())
                .append(", ")
                .bind(step.status.as_str())
                .append(", ")
                .bind(step.flow_step_id.as_str())
                .append(", ")
                .bind(step.attempt_generation)
                .append(", ")
                .bind(step.selected_handle.as_deref())
                .append(", ")
                .bind(step.result.clone())
                .append(", ")
                .bind(step.result_digest.as_ref().map(Sha256Digest::as_str))
                .append(", ")
                .bind(step.error.as_deref())
                .append(", ")
                .bind(serde_json::to_value(&step.evidence_references)?)
                .append(", ")
                .bind(step.last_flow_sequence)
                .append(", ")
                .bind(step.updated_at)
                .append(")"),
        )
        .await?,
    )
}

async fn persist_run(
    transaction: &a3s_orm::PostgresTransaction,
    run: &WorkflowRun,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("update workflow_runs set flow_runtime_build_id = ")
            .bind(run.flow_runtime_build_id.as_deref())
            .append(", status = ")
            .bind(run.status.as_str())
            .append(", last_flow_sequence = ")
            .bind(run.last_flow_sequence)
            .append(", output = ")
            .bind(run.output.clone())
            .append(", output_digest = ")
            .bind(run.output_digest.as_ref().map(Sha256Digest::as_str))
            .append(", error = ")
            .bind(run.error.as_deref())
            .append(", aggregate_version = ")
            .bind(run.aggregate_version)
            .append(", updated_at = ")
            .bind(run.updated_at)
            .append(", started_at = ")
            .bind(run.started_at)
            .append(", cancellation_requested_at = ")
            .bind(run.cancellation_requested_at)
            .append(", cancellation_reason = ")
            .bind(run.cancellation_reason.as_deref())
            .append(", finished_at = ")
            .bind(run.finished_at)
            .append(" where organization_id = ")
            .bind(run.organization_id.as_uuid())
            .append(" and id = ")
            .bind(run.id.as_uuid())
            .append(" and aggregate_version = ")
            .bind(expected_version),
    )
    .await?;
    match rows {
        1 => Ok(()),
        0 => Err(RepositoryError::Conflict("WorkflowRun changed concurrently".into()).into()),
        rows => Err(PostgresPersistenceError::Invariant(format!(
            "projecting WorkflowRun affected {rows} rows"
        ))),
    }
}

async fn persist_step(
    transaction: &a3s_orm::PostgresTransaction,
    step: &WorkflowStepProjection,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("update workflow_step_projections set status = ")
            .bind(step.status.as_str())
            .append(", attempt_generation = ")
            .bind(step.attempt_generation)
            .append(", selected_handle = ")
            .bind(step.selected_handle.as_deref())
            .append(", result = ")
            .bind(step.result.clone())
            .append(", result_digest = ")
            .bind(step.result_digest.as_ref().map(Sha256Digest::as_str))
            .append(", error = ")
            .bind(step.error.as_deref())
            .append(", evidence_references = ")
            .bind(serde_json::to_value(&step.evidence_references)?)
            .append(", last_flow_sequence = ")
            .bind(step.last_flow_sequence)
            .append(", updated_at = ")
            .bind(step.updated_at)
            .append(" where organization_id = ")
            .bind(step.organization_id.as_uuid())
            .append(" and workflow_run_id = ")
            .bind(step.workflow_run_id.as_uuid())
            .append(" and step_id = ")
            .bind(step.step_id.as_str()),
    )
    .await?;
    require_one_row("WorkflowStepProjection", rows)
}

async fn find_run_row(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    workflow_run_id: WorkflowRunId,
    for_update: bool,
) -> Result<Option<WorkflowRunRow>, PostgresPersistenceError> {
    let mut query = sql_query::<WorkflowRunRow>(SELECT_RUNS)
        .append(" where r.organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and r.id = ")
        .bind(workflow_run_id.as_uuid());
    if for_update {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query).await
}

async fn load_record(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    workflow_run_id: WorkflowRunId,
    for_update: bool,
) -> Result<WorkflowRunRecord, PostgresPersistenceError> {
    let row = find_run_row(transaction, organization_id, workflow_run_id, for_update)
        .await?
        .ok_or(PostgresPersistenceError::Repository(
            RepositoryError::NotFound,
        ))?;
    decode_record(transaction, row).await
}

async fn decode_record(
    transaction: &a3s_orm::PostgresTransaction,
    row: WorkflowRunRow,
) -> Result<WorkflowRunRecord, PostgresPersistenceError> {
    let run = decode_run(row)?;
    let rows = fetch_all::<WorkflowStepRow, _>(
        transaction,
        sql_query::<WorkflowStepRow>(SELECT_STEPS)
            .append(" where organization_id = ")
            .bind(run.organization_id.as_uuid())
            .append(" and workflow_run_id = ")
            .bind(run.id.as_uuid()),
    )
    .await?;
    let mut by_id = rows
        .into_iter()
        .map(decode_step)
        .map(|result| result.map(|step| (step.step_id.clone(), step)))
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let mut steps = Vec::with_capacity(by_id.len());
    for planned in &run.execution_input.plan.steps {
        steps.push(by_id.remove(&planned.id).ok_or_else(|| {
            PostgresPersistenceError::Invariant(format!(
                "stored WorkflowRun is missing step {:?}",
                planned.id
            ))
        })?);
    }
    if !by_id.is_empty() {
        return Err(PostgresPersistenceError::Invariant(
            "stored WorkflowRun contains unplanned step projections".into(),
        ));
    }
    let record = WorkflowRunRecord { run, steps };
    record.validate().map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored WorkflowRun is invalid: {error}"))
    })?;
    Ok(record)
}

fn decode_run(row: WorkflowRunRow) -> Result<WorkflowRun, PostgresPersistenceError> {
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
        cancellation_reason: row.cancellation_reason,
        finished_at: row.finished_at,
    }
    .restore()
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored WorkflowRun is invalid: {error}"))
    })
}

fn decode_step(row: WorkflowStepRow) -> Result<WorkflowStepProjection, PostgresPersistenceError> {
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

fn validate_cancellation_transition(
    existing: &WorkflowRunRecord,
    next: &WorkflowRunRecord,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    if existing.run.aggregate_version != expected_version || existing.steps != next.steps {
        return Err(RepositoryError::Conflict(
            "WorkflowRun changed while cancellation was requested".into(),
        )
        .into());
    }
    let mut candidate = existing.run.clone();
    let requested_at = next.run.cancellation_requested_at.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "WorkflowRun cancellation is missing its request time".into(),
        )
    })?;
    candidate
        .request_cancellation(next.run.cancellation_reason.clone(), requested_at)
        .map_err(PostgresPersistenceError::Invariant)?;
    if candidate != next.run {
        return Err(RepositoryError::Conflict(
            "WorkflowRun cancellation transition drifted".into(),
        )
        .into());
    }
    Ok(())
}

fn validate_projection_transition(
    existing: &WorkflowRunRecord,
    next: &WorkflowRunRecord,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    if existing.run.aggregate_version != expected_version
        || next.run.aggregate_version
            != expected_version.checked_add(1).ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "WorkflowRun aggregate version overflowed".into(),
                )
            })?
        || next.run.last_flow_sequence <= existing.run.last_flow_sequence
        || !same_run_authority(&existing.run, &next.run)
        || existing.steps.len() != next.steps.len()
    {
        return Err(RepositoryError::Conflict(
            "WorkflowRun projection transition conflicts with stored state".into(),
        )
        .into());
    }
    for current in &existing.steps {
        let projected = next
            .steps
            .iter()
            .find(|step| step.step_id == current.step_id)
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant("WorkflowRun projection lost a step".into())
            })?;
        if current.organization_id != projected.organization_id
            || current.project_id != projected.project_id
            || current.workflow_run_id != projected.workflow_run_id
            || current.kind != projected.kind
            || current.flow_step_id != projected.flow_step_id
            || projected.last_flow_sequence < current.last_flow_sequence
            || (current.status.is_terminal() && current != projected)
        {
            return Err(RepositoryError::Conflict(format!(
                "Workflow step {:?} projection transition conflicts with stored state",
                current.step_id
            ))
            .into());
        }
    }
    Ok(())
}

fn same_run_authority(left: &WorkflowRun, right: &WorkflowRun) -> bool {
    left.organization_id == right.organization_id
        && left.project_id == right.project_id
        && left.id == right.id
        && left.workflow_goal_id == right.workflow_goal_id
        && left.plan_revision_id == right.plan_revision_id
        && left.plan_digest == right.plan_digest
        && left.operation_id == right.operation_id
        && left.flow_run_id == right.flow_run_id
        && left.execution_input == right.execution_input
        && left.execution_input_digest == right.execution_input_digest
        && left.requested_by == right.requested_by
        && left.requested_at == right.requested_at
}

async fn store_run_audit(
    transaction: &a3s_orm::PostgresTransaction,
    record: &WorkflowRunRecord,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    action: &'static str,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: record.run.organization_id.as_uuid(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: record.run.id.as_uuid(),
            occurred_at: record.run.updated_at,
            request_id,
            details: serde_json::json!({
                "projectId": record.run.project_id,
                "workflowGoalId": record.run.workflow_goal_id,
                "planRevisionId": record.run.plan_revision_id,
                "planDigest": record.run.plan_digest,
                "operationId": record.run.operation_id,
                "flowRunId": record.run.flow_run_id,
                "executionInputDigest": record.run.execution_input_digest,
                "status": record.run.status,
                "deadlineAt": record.run.execution_input.deadline_at,
                "cancellationReason": record.run.cancellation_reason,
            }),
        },
    )
    .await
}

fn parse_digest(value: String, label: &str) -> Result<Sha256Digest, PostgresPersistenceError> {
    Sha256Digest::parse(value).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored WorkflowRun {label} digest is invalid: {error}"
        ))
    })
}

struct WorkflowRunRow {
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
    cancellation_reason: Option<String>,
    finished_at: Option<DateTime<Utc>>,
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
            cancellation_reason: decode(row, 22)?,
            finished_at: decode(row, 23)?,
        })
    }
}

struct WorkflowStepRow {
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

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
