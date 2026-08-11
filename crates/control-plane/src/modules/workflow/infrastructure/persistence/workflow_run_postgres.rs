mod rows;
mod schema;

use self::rows::{
    decode_run, decode_step, WorkflowRunRow, WorkflowRunSelection, WorkflowStepRow,
    WorkflowStepSelection,
};
use self::schema::{OperationRequests, WorkflowRuns, WorkflowStepProjections};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError, WorkflowRunId,
};
use crate::modules::workflow::domain::repositories::WorkflowRunWriteReference;
use crate::modules::workflow::domain::{
    CancelWorkflowRunWrite, CreateWorkflowRunWrite, IWorkflowRunRepository, WorkflowRun,
    WorkflowRunRecord, WorkflowStepProjection, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
};
use a3s_orm::{insert_into, select_from, update_table, OrderDirection, PostgresExecutor};
use async_trait::async_trait;
use uuid::Uuid;

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
                        run_select()
                            .filter(WorkflowRuns::organization_id().eq(organization_id.as_uuid()))
                            .filter(WorkflowRuns::project_id().eq(project_id.as_uuid()))
                            .order_by(WorkflowRuns::requested_at(), OrderDirection::Desc)
                            .order_by(WorkflowRuns::id(), OrderDirection::Desc)
                            .limit(limit as u64),
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
                        run_select()
                            .filter(WorkflowRuns::status().ne("completed"))
                            .filter(WorkflowRuns::status().ne("failed"))
                            .filter(WorkflowRuns::status().ne("cancelled"))
                            .filter(WorkflowRuns::status().ne("timed_out"))
                            .order_by(WorkflowRuns::requested_at(), OrderDirection::Asc)
                            .order_by(WorkflowRuns::id(), OrderDirection::Asc)
                            .limit(limit as u64),
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
            insert_into::<OperationRequests>()
                .value(OperationRequests::operation_id(), operation.id.as_uuid())
                .value(
                    OperationRequests::organization_id(),
                    operation.organization_id.as_uuid(),
                )
                .value(OperationRequests::subject_kind(), operation.subject.kind())
                .value(OperationRequests::subject_id(), operation.subject.id())
                .value(
                    OperationRequests::workflow_name(),
                    operation.workflow.name(),
                )
                .value(
                    OperationRequests::workflow_version(),
                    operation.workflow.version(),
                )
                .value(OperationRequests::input(), operation.input)
                .value(OperationRequests::requested_at(), operation.requested_at),
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
            insert_into::<WorkflowRuns>()
                .value(
                    WorkflowRuns::organization_id(),
                    run.organization_id.as_uuid(),
                )
                .value(WorkflowRuns::project_id(), run.project_id.as_uuid())
                .value(WorkflowRuns::id(), run.id.as_uuid())
                .value(
                    WorkflowRuns::workflow_goal_id(),
                    run.workflow_goal_id.as_uuid(),
                )
                .value(
                    WorkflowRuns::plan_revision_id(),
                    run.plan_revision_id.as_uuid(),
                )
                .value(WorkflowRuns::plan_digest(), run.plan_digest.as_str())
                .value(WorkflowRuns::operation_id(), run.operation_id.as_uuid())
                .value(WorkflowRuns::flow_run_id(), run.flow_run_id.as_str())
                .value(
                    WorkflowRuns::flow_runtime_build_id(),
                    run.flow_runtime_build_id.clone(),
                )
                .value(WorkflowRuns::execution_input(), execution_input)
                .value(
                    WorkflowRuns::execution_input_digest(),
                    run.execution_input_digest.as_str(),
                )
                .value(WorkflowRuns::status(), run.status.as_str())
                .value(WorkflowRuns::last_flow_sequence(), run.last_flow_sequence)
                .value(WorkflowRuns::output(), run.output.clone())
                .value(
                    WorkflowRuns::output_digest(),
                    run.output_digest
                        .as_ref()
                        .map(|digest| digest.as_str().to_owned()),
                )
                .value(WorkflowRuns::error(), run.error.clone())
                .value(WorkflowRuns::aggregate_version(), run.aggregate_version)
                .value(WorkflowRuns::requested_by(), run.requested_by.as_uuid())
                .value(WorkflowRuns::requested_at(), run.requested_at)
                .value(WorkflowRuns::updated_at(), run.updated_at)
                .value(WorkflowRuns::started_at(), run.started_at)
                .value(
                    WorkflowRuns::cancellation_requested_at(),
                    run.cancellation_requested_at,
                )
                .value(
                    WorkflowRuns::cancellation_reason(),
                    run.cancellation_reason.clone(),
                )
                .value(WorkflowRuns::finished_at(), run.finished_at),
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
            insert_into::<WorkflowStepProjections>()
                .value(
                    WorkflowStepProjections::organization_id(),
                    step.organization_id.as_uuid(),
                )
                .value(
                    WorkflowStepProjections::project_id(),
                    step.project_id.as_uuid(),
                )
                .value(
                    WorkflowStepProjections::workflow_run_id(),
                    step.workflow_run_id.as_uuid(),
                )
                .value(WorkflowStepProjections::step_id(), step.step_id.as_str())
                .value(WorkflowStepProjections::kind(), step.kind.as_str())
                .value(WorkflowStepProjections::status(), step.status.as_str())
                .value(
                    WorkflowStepProjections::flow_step_id(),
                    step.flow_step_id.as_str(),
                )
                .value(
                    WorkflowStepProjections::attempt_generation(),
                    step.attempt_generation,
                )
                .value(
                    WorkflowStepProjections::selected_handle(),
                    step.selected_handle.clone(),
                )
                .value(WorkflowStepProjections::result(), step.result.clone())
                .value(
                    WorkflowStepProjections::result_digest(),
                    step.result_digest
                        .as_ref()
                        .map(|digest| digest.as_str().to_owned()),
                )
                .value(WorkflowStepProjections::error(), step.error.clone())
                .value(
                    WorkflowStepProjections::evidence_references(),
                    serde_json::to_value(&step.evidence_references)?,
                )
                .value(
                    WorkflowStepProjections::last_flow_sequence(),
                    step.last_flow_sequence,
                )
                .value(WorkflowStepProjections::updated_at(), step.updated_at),
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
        update_table::<WorkflowRuns>()
            .set(
                WorkflowRuns::flow_runtime_build_id(),
                run.flow_runtime_build_id.clone(),
            )
            .set(WorkflowRuns::status(), run.status.as_str())
            .set(WorkflowRuns::last_flow_sequence(), run.last_flow_sequence)
            .set(WorkflowRuns::output(), run.output.clone())
            .set(
                WorkflowRuns::output_digest(),
                run.output_digest
                    .as_ref()
                    .map(|digest| digest.as_str().to_owned()),
            )
            .set(WorkflowRuns::error(), run.error.clone())
            .set(WorkflowRuns::aggregate_version(), run.aggregate_version)
            .set(WorkflowRuns::updated_at(), run.updated_at)
            .set(WorkflowRuns::started_at(), run.started_at)
            .set(
                WorkflowRuns::cancellation_requested_at(),
                run.cancellation_requested_at,
            )
            .set(
                WorkflowRuns::cancellation_reason(),
                run.cancellation_reason.clone(),
            )
            .set(WorkflowRuns::finished_at(), run.finished_at)
            .filter(WorkflowRuns::organization_id().eq(run.organization_id.as_uuid()))
            .filter(WorkflowRuns::id().eq(run.id.as_uuid()))
            .filter(WorkflowRuns::aggregate_version().eq(expected_version)),
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
        update_table::<WorkflowStepProjections>()
            .set(WorkflowStepProjections::status(), step.status.as_str())
            .set(
                WorkflowStepProjections::attempt_generation(),
                step.attempt_generation,
            )
            .set(
                WorkflowStepProjections::selected_handle(),
                step.selected_handle.clone(),
            )
            .set(WorkflowStepProjections::result(), step.result.clone())
            .set(
                WorkflowStepProjections::result_digest(),
                step.result_digest
                    .as_ref()
                    .map(|digest| digest.as_str().to_owned()),
            )
            .set(WorkflowStepProjections::error(), step.error.clone())
            .set(
                WorkflowStepProjections::evidence_references(),
                serde_json::to_value(&step.evidence_references)?,
            )
            .set(
                WorkflowStepProjections::last_flow_sequence(),
                step.last_flow_sequence,
            )
            .set(WorkflowStepProjections::updated_at(), step.updated_at)
            .filter(WorkflowStepProjections::organization_id().eq(step.organization_id.as_uuid()))
            .filter(WorkflowStepProjections::workflow_run_id().eq(step.workflow_run_id.as_uuid()))
            .filter(WorkflowStepProjections::step_id().eq(step.step_id.as_str())),
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
    let mut query = run_select()
        .filter(WorkflowRuns::organization_id().eq(organization_id.as_uuid()))
        .filter(WorkflowRuns::id().eq(workflow_run_id.as_uuid()));
    if for_update {
        query = query.for_update();
    }
    fetch_optional(transaction, query).await
}

fn run_select() -> a3s_orm::query::SelectQuery<WorkflowRuns, WorkflowRunRow> {
    select_from::<WorkflowRuns>().select(WorkflowRunSelection)
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
        select_from::<WorkflowStepProjections>()
            .select(WorkflowStepSelection)
            .filter(WorkflowStepProjections::organization_id().eq(run.organization_id.as_uuid()))
            .filter(WorkflowStepProjections::workflow_run_id().eq(run.id.as_uuid())),
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
