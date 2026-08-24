use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest, WorkflowGoalId,
};
use crate::modules::workflow::domain::repositories::WorkflowGoalWriteReference;
use crate::modules::workflow::domain::{
    CreateWorkflowGoalWrite, IWorkflowGoalRepository, PlanRevision, WorkflowGoal,
    WorkflowGoalRecord,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresWorkflowGoalRepository {
    executor: PostgresExecutor,
}

impl PostgresWorkflowGoalRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

struct WorkflowGoalRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    canonical_acl: String,
    contract_digest: String,
    input_digest: String,
    plan_revision_id: Uuid,
    plan_digest: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for WorkflowGoalRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            id: decode(row, 2)?,
            canonical_acl: decode(row, 3)?,
            contract_digest: decode(row, 4)?,
            input_digest: decode(row, 5)?,
            plan_revision_id: decode(row, 6)?,
            plan_digest: decode(row, 7)?,
            created_by: decode(row, 8)?,
            created_at: decode(row, 9)?,
        })
    }
}

struct PlanRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    workflow_goal_id: Uuid,
    id: Uuid,
    canonical_plan: String,
    plan_digest: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for PlanRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            workflow_goal_id: decode(row, 2)?,
            id: decode(row, 3)?,
            canonical_plan: decode(row, 4)?,
            plan_digest: decode(row, 5)?,
            created_by: decode(row, 6)?,
            created_at: decode(row, 7)?,
        })
    }
}

#[async_trait]
impl IWorkflowGoalRepository for PostgresWorkflowGoalRepository {
    async fn create(
        &self,
        write: CreateWorkflowGoalWrite,
    ) -> Result<IdempotentWrite<WorkflowGoalRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<WorkflowGoalWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_record(transaction, reference.value).await?,
                            replayed: true,
                        });
                    }
                    write
                        .record
                        .goal
                        .validate(&write.record.plan_revision)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let insertion = async {
                        insert_goal(transaction, &write.record.goal).await?;
                        insert_plan(transaction, &write.record.plan_revision).await
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "WorkflowGoal identity already exists".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_goal_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        write.request_id,
                    )
                    .await?;
                    let reference = WorkflowGoalWriteReference {
                        organization_id: write.record.goal.organization_id,
                        workflow_goal_id: write.record.goal.id,
                        plan_revision_id: write.record.plan_revision.id,
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

    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<WorkflowGoalRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(replay) =
                        idempotency_replay::<WorkflowGoalWriteReference>(transaction, &idempotency)
                            .await?
                    else {
                        return Ok(None);
                    };
                    load_record(transaction, replay.value).await.map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        goal_id: WorkflowGoalId,
    ) -> Result<Option<WorkflowGoalRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let row = fetch_optional::<WorkflowGoalRow, _>(
                        transaction,
                        goal_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(goal_id.as_uuid()),
                    )
                    .await?;
                    match row {
                        Some(row) => decode_goal_record(transaction, row).await.map(Some),
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
    ) -> Result<Vec<WorkflowGoalRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<WorkflowGoalRow, _>(
                        transaction,
                        goal_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(project_id.as_uuid())
                            .append(" order by created_at desc, id asc"),
                    )
                    .await?;
                    let mut values = Vec::with_capacity(rows.len());
                    for row in rows {
                        values.push(decode_goal_record(transaction, row).await?);
                    }
                    Ok(values)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_plan_revision(
        &self,
        organization_id: OrganizationId,
        goal_id: WorkflowGoalId,
        plan_revision_id: PlanRevisionId,
    ) -> Result<Option<PlanRevision>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_optional::<PlanRevisionRow, _>(
                        transaction,
                        plan_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and workflow_goal_id = ")
                            .bind(goal_id.as_uuid())
                            .append(" and id = ")
                            .bind(plan_revision_id.as_uuid()),
                    )
                    .await?
                    .map(decode_plan)
                    .transpose()
                })
            })
            .await
            .map_err(transaction_error)
    }
}

fn goal_select() -> a3s_orm::SqlQuery<WorkflowGoalRow> {
    sql_query::<WorkflowGoalRow>(
        "select organization_id, project_id, id, canonical_acl, contract_digest, input_digest, plan_revision_id, plan_digest, created_by, created_at from workflow_goals",
    )
}

fn plan_select() -> a3s_orm::SqlQuery<PlanRevisionRow> {
    sql_query::<PlanRevisionRow>(
        "select organization_id, project_id, workflow_goal_id, id, canonical_plan, plan_digest, created_by, created_at from workflow_plan_revisions",
    )
}

async fn load_record(
    transaction: &a3s_orm::PostgresTransaction,
    reference: WorkflowGoalWriteReference,
) -> Result<WorkflowGoalRecord, PostgresPersistenceError> {
    let row = fetch_optional::<WorkflowGoalRow, _>(
        transaction,
        goal_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and id = ")
            .bind(reference.workflow_goal_id.as_uuid()),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("WorkflowGoal replay target is missing".into())
    })?;
    if row.plan_revision_id != reference.plan_revision_id.as_uuid() {
        return Err(PostgresPersistenceError::Invariant(
            "WorkflowGoal replay plan target does not match".into(),
        ));
    }
    decode_goal_record(transaction, row).await
}

async fn decode_goal_record(
    transaction: &a3s_orm::PostgresTransaction,
    row: WorkflowGoalRow,
) -> Result<WorkflowGoalRecord, PostgresPersistenceError> {
    let plan_row = fetch_optional::<PlanRevisionRow, _>(
        transaction,
        plan_select()
            .append(" where organization_id = ")
            .bind(row.organization_id)
            .append(" and workflow_goal_id = ")
            .bind(row.id)
            .append(" and id = ")
            .bind(row.plan_revision_id),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("WorkflowGoal plan target is missing".into())
    })?;
    let plan_revision = decode_plan(plan_row)?;
    let goal = WorkflowGoal::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        WorkflowGoalId::from_uuid(row.id),
        &row.canonical_acl,
        &row.contract_digest,
        &row.input_digest,
        PlanRevisionId::from_uuid(row.plan_revision_id),
        Sha256Digest::parse(row.plan_digest).map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored WorkflowGoal plan digest is invalid: {error}"
            ))
        })?,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
        &plan_revision,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored WorkflowGoal is invalid: {error}"))
    })?;
    Ok(WorkflowGoalRecord {
        goal,
        plan_revision,
    })
}

fn decode_plan(row: PlanRevisionRow) -> Result<PlanRevision, PostgresPersistenceError> {
    PlanRevision::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        WorkflowGoalId::from_uuid(row.workflow_goal_id),
        PlanRevisionId::from_uuid(row.id),
        &row.canonical_plan,
        &row.plan_digest,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored PlanRevision is invalid: {error}"))
    })
}

async fn insert_goal(
    transaction: &a3s_orm::PostgresTransaction,
    goal: &WorkflowGoal,
) -> Result<(), PostgresPersistenceError> {
    let spec = goal.contract.spec();
    require_one_row(
        "WorkflowGoal",
        execute(
            transaction,
            sql_query::<()>("insert into workflow_goals (organization_id, project_id, id, name, contract_schema, canonical_acl, contract_digest, input_digest, workflow_definition_id, workflow_revision_id, workflow_digest, ontology_id, ontology_revision_id, ontology_digest, environment_id, plan_revision_id, plan_digest, created_by, created_at) values (")
                .bind(goal.organization_id.as_uuid())
                .append(", ")
                .bind(goal.project_id.as_uuid())
                .append(", ")
                .bind(goal.id.as_uuid())
                .append(", ")
                .bind(spec.name.as_str())
                .append(", ")
                .bind(crate::modules::workflow::domain::WORKFLOW_GOAL_SCHEMA)
                .append(", ")
                .bind(goal.contract.canonical_acl())
                .append(", ")
                .bind(goal.contract.digest().as_str())
                .append(", ")
                .bind(goal.contract.input_digest().as_str())
                .append(", ")
                .bind(spec.workflow_definition_id.as_uuid())
                .append(", ")
                .bind(spec.workflow_revision_id.as_uuid())
                .append(", ")
                .bind(spec.workflow_digest.as_str())
                .append(", ")
                .bind(spec.ontology_id.as_uuid())
                .append(", ")
                .bind(spec.ontology_revision_id.as_uuid())
                .append(", ")
                .bind(spec.ontology_digest.as_str())
                .append(", ")
                .bind(spec.environment_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(goal.plan_revision_id.as_uuid())
                .append(", ")
                .bind(goal.plan_digest.as_str())
                .append(", ")
                .bind(goal.created_by.as_uuid())
                .append(", ")
                .bind(goal.created_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_plan(
    transaction: &a3s_orm::PostgresTransaction,
    plan: &PlanRevision,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "PlanRevision",
        execute(
            transaction,
            sql_query::<()>("insert into workflow_plan_revisions (organization_id, project_id, workflow_goal_id, id, plan_schema, compiler_revision, canonical_plan, plan_digest, created_by, created_at) values (")
                .bind(plan.organization_id.as_uuid())
                .append(", ")
                .bind(plan.project_id.as_uuid())
                .append(", ")
                .bind(plan.workflow_goal_id.as_uuid())
                .append(", ")
                .bind(plan.id.as_uuid())
                .append(", ")
                .bind(plan.plan.schema.as_str())
                .append(", ")
                .bind(plan.plan.compiler_revision.as_str())
                .append(", ")
                .bind(plan.canonical_plan.as_str())
                .append(", ")
                .bind(plan.digest.as_str())
                .append(", ")
                .bind(plan.created_by.as_uuid())
                .append(", ")
                .bind(plan.created_at)
                .append(")"),
        )
        .await?,
    )
}

async fn store_goal_audit(
    transaction: &a3s_orm::PostgresTransaction,
    record: &WorkflowGoalRecord,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: record.goal.organization_id.as_uuid(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action: "workflow.goal.compiled",
            aggregate_id: record.goal.id.as_uuid(),
            occurred_at: record.goal.created_at,
            request_id,
            attribution_scope: AuditWrite::project_attribution(record.goal.project_id, None),
            details: serde_json::json!({
                "projectId": record.goal.project_id,
                "planRevisionId": record.plan_revision.id,
                "planDigest": record.plan_revision.digest,
                "workflowRevisionId": record.plan_revision.plan.workflow_revision_id,
                "ontologyRevisionId": record.plan_revision.plan.ontology_revision_id,
                "inputDigest": record.plan_revision.plan.input_digest,
                "compilerRevision": record.plan_revision.plan.compiler_revision,
                "semanticContractSetDigest": record.plan_revision.plan.semantic_contract_set_digest,
                "variableContractDigest": record.plan_revision.plan.variable_contract_digest,
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
