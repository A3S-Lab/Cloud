use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::executions::domain::{
    CreateExecutionTemplateRevision, ExecutionTemplateDefinition, ExecutionTemplateRevision,
    IExecutionTemplateRepository,
};
use crate::modules::shared_kernel::domain::{
    ExecutionTemplateId, ExecutionTemplateRevisionId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_REVISIONS: &str = "select revision.organization_id, revision.project_id, revision.template_id, revision.revision_id, revision.canonical_acl, revision.definition_digest, revision.created_by, revision.created_at from execution_template_revisions revision";

#[derive(Clone)]
pub struct PostgresExecutionTemplateRepository {
    executor: PostgresExecutor,
}

impl PostgresExecutionTemplateRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IExecutionTemplateRepository for PostgresExecutionTemplateRepository {
    async fn replay_create(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<ExecutionTemplateRevision>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let replay =
                        idempotency_replay::<ExecutionTemplateRevision>(transaction, &idempotency)
                            .await?;
                    if let Some(replay) = &replay {
                        replay.value.validate().map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "ExecutionTemplate replay target is invalid: {error}"
                            ))
                        })?;
                    }
                    Ok(replay)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create(
        &self,
        write: CreateExecutionTemplateRevision,
    ) -> Result<IdempotentWrite<ExecutionTemplateRevision>, RepositoryError> {
        let revision = write.revision.restore().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = idempotency_replay::<ExecutionTemplateRevision>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        replay.value.validate().map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "ExecutionTemplate replay target is invalid: {error}"
                            ))
                        })?;
                        return Ok(replay);
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into execution_template_revisions (organization_id, project_id, template_id, revision_id, canonical_acl, definition_digest, created_by, created_at) values (",
                        )
                        .bind(revision.organization_id.as_uuid())
                        .append(", ")
                        .bind(revision.project_id.as_uuid())
                        .append(", ")
                        .bind(revision.template_id.as_uuid())
                        .append(", ")
                        .bind(revision.revision_id.as_uuid())
                        .append(", ")
                        .bind(revision.definition.canonical_acl())
                        .append(", ")
                        .bind(revision.definition.digest().as_str())
                        .append(", ")
                        .bind(revision.created_by.as_uuid())
                        .append(", ")
                        .bind(revision.created_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "publishing ExecutionTemplate affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "execution template revision identity is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: revision.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "execution.template.published",
                            aggregate_id: revision.template_id.as_uuid(),
                            occurred_at: revision.created_at,
                            request_id: write.request_id,
                            attribution_scope: AuditWrite::project_attribution(
                                revision.project_id,
                                None,
                            ),
                            details: serde_json::json!({
                                "projectId": revision.project_id,
                                "revisionId": revision.revision_id,
                                "definitionDigest": revision.definition.digest(),
                                "capability": crate::modules::executions::domain::EXECUTION_TEMPLATE_CAPABILITY,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &revision).await?;
                    Ok(IdempotentWrite {
                        value: revision,
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
        project_id: ProjectId,
        template_id: ExecutionTemplateId,
        revision_id: ExecutionTemplateRevisionId,
    ) -> Result<Option<ExecutionTemplateRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<ExecutionTemplateRevisionRow>(SELECT_REVISIONS)
                    .append(" where revision.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and revision.project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and revision.template_id = ")
                    .bind(template_id.as_uuid())
                    .append(" and revision.revision_id = ")
                    .bind(revision_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_revision)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<ExecutionTemplateRevision>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<ExecutionTemplateRevisionRow>(SELECT_REVISIONS)
                    .append(" where revision.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and revision.project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" order by revision.created_at desc, revision.template_id desc, revision.revision_id desc limit ")
                    .bind(limit),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_revision)
            .collect()
    }
}

struct ExecutionTemplateRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    template_id: Uuid,
    revision_id: Uuid,
    canonical_acl: String,
    definition_digest: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for ExecutionTemplateRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            template_id: decode(row, 2)?,
            revision_id: decode(row, 3)?,
            canonical_acl: decode(row, 4)?,
            definition_digest: decode(row, 5)?,
            created_by: decode(row, 6)?,
            created_at: decode(row, 7)?,
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

fn decode_revision(
    row: ExecutionTemplateRevisionRow,
) -> Result<ExecutionTemplateRevision, RepositoryError> {
    ExecutionTemplateRevision {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        template_id: ExecutionTemplateId::from_uuid(row.template_id),
        revision_id: ExecutionTemplateRevisionId::from_uuid(row.revision_id),
        definition: ExecutionTemplateDefinition::restore(
            &row.canonical_acl,
            &row.definition_digest,
        )
        .map_err(|error| {
            corrupt(format!(
                "stored ExecutionTemplate definition is invalid: {error}"
            ))
        })?,
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
    }
    .restore()
    .map_err(|error| {
        corrupt(format!(
            "stored ExecutionTemplate revision is invalid: {error}"
        ))
    })
}

fn corrupt(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(message.into())
}
