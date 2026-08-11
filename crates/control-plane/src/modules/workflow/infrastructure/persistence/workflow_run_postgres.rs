use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::operations::infrastructure::persistence::enqueue_operation;
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OperationId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest, WorkflowGoalId, WorkflowRunId,
};
use crate::modules::workflow::application::workflow_run_operation;
use crate::modules::workflow::domain::repositories::WorkflowRunWriteReference;
use crate::modules::workflow::domain::{
    IWorkflowRunRepository, StartWorkflowRunWrite, WorkflowRun,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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

struct WorkflowRunRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    workflow_goal_id: Uuid,
    plan_revision_id: Uuid,
    plan_digest: String,
    operation_id: Uuid,
    requested_by: Uuid,
    requested_at: DateTime<Utc>,
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
            requested_by: decode(row, 7)?,
            requested_at: decode(row, 8)?,
        })
    }
}

#[async_trait]
impl IWorkflowRunRepository for PostgresWorkflowRunRepository {
    async fn start(
        &self,
        write: StartWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRun>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<WorkflowRunWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_run(transaction, reference.value).await?,
                            replayed: true,
                        });
                    }
                    write
                        .run
                        .validate_identity()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let expected_operation = workflow_run_operation(&write.run)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    if !write.operation.has_same_definition(&expected_operation) {
                        return Err(PostgresPersistenceError::Invariant(
                            "WorkflowRun write contains another Operation request".into(),
                        ));
                    }
                    enqueue_operation(transaction, write.operation).await?;
                    let inserted = insert_run(transaction, &write.run).await;
                    match inserted {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "WorkflowRun identity already exists".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_run_audit(
                        transaction,
                        &write.run,
                        write.actor_principal_id,
                        write.request_id,
                    )
                    .await?;
                    let reference = WorkflowRunWriteReference {
                        organization_id: write.run.organization_id,
                        workflow_run_id: write.run.id,
                    };
                    store_idempotency(transaction, &write.idempotency, &reference).await?;
                    Ok(IdempotentWrite {
                        value: write.run,
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
    ) -> Result<Option<WorkflowRun>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_optional::<WorkflowRunRow, _>(
                        transaction,
                        run_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(workflow_run_id.as_uuid()),
                    )
                    .await?
                    .map(decode_run)
                    .transpose()
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowRun>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_all::<WorkflowRunRow, _>(
                        transaction,
                        run_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(project_id.as_uuid())
                            .append(" order by requested_at desc, id asc"),
                    )
                    .await?
                    .into_iter()
                    .map(decode_run)
                    .collect()
                })
            })
            .await
            .map_err(transaction_error)
    }
}

fn run_select() -> a3s_orm::SqlQuery<WorkflowRunRow> {
    sql_query::<WorkflowRunRow>(
        "select organization_id, project_id, id, workflow_goal_id, plan_revision_id, plan_digest, operation_id, requested_by, requested_at from workflow_runs",
    )
}

async fn load_run(
    transaction: &a3s_orm::PostgresTransaction,
    reference: WorkflowRunWriteReference,
) -> Result<WorkflowRun, PostgresPersistenceError> {
    fetch_optional::<WorkflowRunRow, _>(
        transaction,
        run_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and id = ")
            .bind(reference.workflow_run_id.as_uuid()),
    )
    .await?
    .map(decode_run)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("WorkflowRun replay target is missing".into())
    })
}

fn decode_run(row: WorkflowRunRow) -> Result<WorkflowRun, PostgresPersistenceError> {
    WorkflowRun::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        WorkflowRunId::from_uuid(row.id),
        WorkflowGoalId::from_uuid(row.workflow_goal_id),
        PlanRevisionId::from_uuid(row.plan_revision_id),
        Sha256Digest::parse(row.plan_digest).map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored WorkflowRun plan digest is invalid: {error}"
            ))
        })?,
        OperationId::from_uuid(row.operation_id),
        PrincipalId::from_uuid(row.requested_by),
        row.requested_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored WorkflowRun is invalid: {error}"))
    })
}

async fn insert_run(
    transaction: &a3s_orm::PostgresTransaction,
    run: &WorkflowRun,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "WorkflowRun",
        execute(
            transaction,
            sql_query::<()>("insert into workflow_runs (organization_id, project_id, id, workflow_goal_id, plan_revision_id, plan_digest, operation_id, requested_by, requested_at) values (")
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
                .bind(run.requested_by.as_uuid())
                .append(", ")
                .bind(run.requested_at)
                .append(")"),
        )
        .await?,
    )
}

async fn store_run_audit(
    transaction: &a3s_orm::PostgresTransaction,
    run: &WorkflowRun,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: run.organization_id.as_uuid(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action: "workflow.run.requested",
            aggregate_id: run.id.as_uuid(),
            occurred_at: run.requested_at,
            request_id,
            details: serde_json::json!({
                "projectId": run.project_id,
                "workflowGoalId": run.workflow_goal_id,
                "planRevisionId": run.plan_revision_id,
                "planDigest": run.plan_digest,
                "operationId": run.operation_id,
            }),
        },
    )
    .await
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
