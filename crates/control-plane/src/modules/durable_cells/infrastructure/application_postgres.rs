use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::durable_cells::domain::{
    CreateDurableCellApplicationWrite, DurableCellApplication, DurableCellApplicationDefinition,
    DurableCellApplicationDesiredState, DurableCellApplicationRecord,
    DurableCellApplicationRevision, DurableCellProjectionIdentity, DurableCellWriteReference,
    IDurableCellApplicationRepository, RequestDurableCellApplicationStateWrite,
    ReviseDurableCellApplicationWrite, DURABLE_CELL_APPLICATION_SCHEMA,
};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, IdempotencyRequest,
    IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError, ResourceName,
    Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_APPLICATIONS: &str = "select organization_id, project_id, environment_id, id, name, desired_state, current_revision_id, current_revision_number, current_definition_digest, aggregate_version, created_by, created_at, updated_at from durable_cell_applications";
const SELECT_REVISIONS: &str = "select organization_id, project_id, environment_id, application_id, id, revision_number, parent_revision_id, parent_definition_digest, definition_schema, canonical_acl, definition_digest, build_run_id, created_by, created_at from durable_cell_application_revisions";

#[derive(Clone)]
pub struct PostgresDurableCellApplicationRepository {
    executor: PostgresExecutor,
}

impl PostgresDurableCellApplicationRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IDurableCellApplicationRepository for PostgresDurableCellApplicationRepository {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DurableCellApplicationRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) =
                        idempotency_replay::<DurableCellWriteReference>(transaction, &idempotency)
                            .await?
                    else {
                        return Ok(None);
                    };
                    load_record(transaction, reference.value).await.map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create(
        &self,
        write: CreateDurableCellApplicationWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) =
                        idempotency_replay::<DurableCellWriteReference>(
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
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let insertion = async {
                        insert_application(transaction, &write.record.application).await?;
                        insert_revision(transaction, &write.record.revision).await
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Durable Cell application name or revision identity is already in use"
                                    .into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    persist_write(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        "durable-cell.application.created",
                        write.request_id,
                        &write.idempotency,
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revise(
        &self,
        write: ReviseDurableCellApplicationWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<DurableCellWriteReference>(
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
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let current = lock_application(
                        transaction,
                        write.record.application.organization_id,
                        write.record.application.project_id,
                        write.record.application.environment_id,
                        write.record.application.id,
                    )
                    .await?;
                    write.validate_against(&current).map_err(|error| {
                        PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                    })?;
                    match insert_revision(transaction, &write.record.revision).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Durable Cell revision identity is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    let updated = execute(
                        transaction,
                        sql_query::<()>(
                            "update durable_cell_applications set current_revision_id = ",
                        )
                        .bind(write.record.application.current_revision_id.as_uuid())
                        .append(", current_revision_number = ")
                        .bind(write.record.application.current_revision_number)
                        .append(", current_definition_digest = ")
                        .bind(write.record.application.current_definition_digest.as_str())
                        .append(", aggregate_version = ")
                        .bind(write.record.application.aggregate_version)
                        .append(", updated_at = ")
                        .bind(write.record.application.updated_at)
                        .append(" where organization_id = ")
                        .bind(write.record.application.organization_id.as_uuid())
                        .append(" and project_id = ")
                        .bind(write.record.application.project_id.as_uuid())
                        .append(" and environment_id = ")
                        .bind(write.record.application.environment_id.as_uuid())
                        .append(" and id = ")
                        .bind(write.record.application.id.as_uuid())
                        .append(" and aggregate_version = ")
                        .bind(write.expected_version)
                        .append(" and current_revision_id = ")
                        .bind(current.current_revision_id.as_uuid()),
                    )
                    .await?;
                    require_updated("revising Durable Cell application", updated)?;
                    persist_write(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        "durable-cell.application.revised",
                        write.request_id,
                        &write.idempotency,
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn request_state(
        &self,
        write: RequestDurableCellApplicationStateWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<DurableCellWriteReference>(
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
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let current = lock_application(
                        transaction,
                        write.record.application.organization_id,
                        write.record.application.project_id,
                        write.record.application.environment_id,
                        write.record.application.id,
                    )
                    .await?;
                    write.validate_against(&current).map_err(|error| {
                        PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                    })?;
                    let stored_revision = fetch_revision(
                        transaction,
                        write.record.revision.organization_id,
                        write.record.revision.project_id,
                        write.record.revision.environment_id,
                        write.record.revision.application_id,
                        write.record.revision.id,
                    )
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "Durable Cell desired-state revision is missing".into(),
                        )
                    })?;
                    if stored_revision != write.record.revision {
                        return Err(PostgresPersistenceError::Invariant(
                            "Durable Cell desired-state revision drifted".into(),
                        ));
                    }
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update durable_cell_applications set desired_state = ")
                            .bind(write.record.application.desired_state.as_str())
                            .append(", aggregate_version = ")
                            .bind(write.record.application.aggregate_version)
                            .append(", updated_at = ")
                            .bind(write.record.application.updated_at)
                            .append(" where organization_id = ")
                            .bind(write.record.application.organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(write.record.application.project_id.as_uuid())
                            .append(" and environment_id = ")
                            .bind(write.record.application.environment_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.application.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and current_revision_id = ")
                            .bind(current.current_revision_id.as_uuid())
                            .append(" and desired_state = ")
                            .bind(current.desired_state.as_str()),
                    )
                    .await?;
                    require_updated("requesting Durable Cell desired state", updated)?;
                    persist_write(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        "durable-cell.application.state-requested",
                        write.request_id,
                        &write.idempotency,
                    )
                    .await?;
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
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
    ) -> Result<Option<DurableCellApplication>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                application_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and id = ")
                    .bind(application_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_application)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<DurableCellApplication>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                application_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" order by name_key asc, id asc limit ")
                    .bind(limit),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(decode_application)
            .collect()
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        revision_id: DurableCellApplicationRevisionId,
    ) -> Result<Option<DurableCellApplicationRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                revision_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" and id = ")
                    .bind(revision_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_revision)
            .transpose()
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        limit: usize,
    ) -> Result<Vec<DurableCellApplicationRevision>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                revision_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" order by revision_number desc, id asc limit ")
                    .bind(limit),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(decode_revision)
            .collect()
    }
}

struct DurableCellApplicationRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    name: String,
    desired_state: String,
    current_revision_id: Uuid,
    current_revision_number: u64,
    current_definition_digest: String,
    aggregate_version: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for DurableCellApplicationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            id: decode(row, 3)?,
            name: decode(row, 4)?,
            desired_state: decode(row, 5)?,
            current_revision_id: decode(row, 6)?,
            current_revision_number: decode(row, 7)?,
            current_definition_digest: decode(row, 8)?,
            aggregate_version: decode(row, 9)?,
            created_by: decode(row, 10)?,
            created_at: decode(row, 11)?,
            updated_at: decode(row, 12)?,
        })
    }
}

struct DurableCellRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
    id: Uuid,
    revision_number: u64,
    parent_revision_id: Option<Uuid>,
    parent_definition_digest: Option<String>,
    definition_schema: String,
    canonical_acl: String,
    definition_digest: String,
    build_run_id: Uuid,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for DurableCellRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            application_id: decode(row, 3)?,
            id: decode(row, 4)?,
            revision_number: decode(row, 5)?,
            parent_revision_id: decode(row, 6)?,
            parent_definition_digest: decode(row, 7)?,
            definition_schema: decode(row, 8)?,
            canonical_acl: decode(row, 9)?,
            definition_digest: decode(row, 10)?,
            build_run_id: decode(row, 11)?,
            created_by: decode(row, 12)?,
            created_at: decode(row, 13)?,
        })
    }
}

fn application_select() -> a3s_orm::SqlQuery<DurableCellApplicationRow> {
    sql_query::<DurableCellApplicationRow>(SELECT_APPLICATIONS)
}

fn revision_select() -> a3s_orm::SqlQuery<DurableCellRevisionRow> {
    sql_query::<DurableCellRevisionRow>(SELECT_REVISIONS)
}

async fn lock_application(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
) -> Result<DurableCellApplication, PostgresPersistenceError> {
    fetch_optional::<DurableCellApplicationRow, _>(
        transaction,
        application_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(environment_id.as_uuid())
            .append(" and id = ")
            .bind(application_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_application)
    .transpose()?
    .ok_or_else(|| PostgresPersistenceError::Repository(RepositoryError::NotFound))
}

async fn fetch_revision(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    revision_id: DurableCellApplicationRevisionId,
) -> Result<Option<DurableCellApplicationRevision>, PostgresPersistenceError> {
    fetch_optional::<DurableCellRevisionRow, _>(
        transaction,
        revision_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(environment_id.as_uuid())
            .append(" and application_id = ")
            .bind(application_id.as_uuid())
            .append(" and id = ")
            .bind(revision_id.as_uuid()),
    )
    .await?
    .map(decode_revision)
    .transpose()
    .map_err(Into::into)
}

async fn load_record(
    transaction: &PostgresTransaction,
    reference: DurableCellWriteReference,
) -> Result<DurableCellApplicationRecord, PostgresPersistenceError> {
    let head = fetch_optional::<DurableCellApplicationRow, _>(
        transaction,
        application_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(reference.environment_id.as_uuid())
            .append(" and id = ")
            .bind(reference.application_id.as_uuid()),
    )
    .await?
    .map(decode_application)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Durable Cell replay application is missing".into())
    })?;
    let revision = fetch_revision(
        transaction,
        reference.organization_id,
        reference.project_id,
        reference.environment_id,
        reference.application_id,
        reference.revision_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Durable Cell replay revision is missing".into())
    })?;
    DurableCellApplicationRecord::replay_snapshot(
        &head,
        revision,
        reference.desired_state,
        reference.aggregate_version,
        reference.updated_at,
    )
    .map_err(PostgresPersistenceError::Invariant)
}

async fn insert_application(
    transaction: &PostgresTransaction,
    application: &DurableCellApplication,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Durable Cell application",
        execute(
            transaction,
            sql_query::<()>("insert into durable_cell_applications (organization_id, project_id, environment_id, id, name, name_key, desired_state, current_revision_id, current_revision_number, current_definition_digest, aggregate_version, created_by, created_at, updated_at) values (")
                .bind(application.organization_id.as_uuid())
                .append(", ")
                .bind(application.project_id.as_uuid())
                .append(", ")
                .bind(application.environment_id.as_uuid())
                .append(", ")
                .bind(application.id.as_uuid())
                .append(", ")
                .bind(application.name.as_str())
                .append(", ")
                .bind(application.name.key())
                .append(", ")
                .bind(application.desired_state.as_str())
                .append(", ")
                .bind(application.current_revision_id.as_uuid())
                .append(", ")
                .bind(application.current_revision_number)
                .append(", ")
                .bind(application.current_definition_digest.as_str())
                .append(", ")
                .bind(application.aggregate_version)
                .append(", ")
                .bind(application.created_by.as_uuid())
                .append(", ")
                .bind(application.created_at)
                .append(", ")
                .bind(application.updated_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_revision(
    transaction: &PostgresTransaction,
    revision: &DurableCellApplicationRevision,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Durable Cell application revision",
        execute(
            transaction,
            sql_query::<()>("insert into durable_cell_application_revisions (organization_id, project_id, environment_id, application_id, id, revision_number, parent_revision_id, parent_definition_digest, definition_schema, canonical_acl, definition_digest, build_run_id, created_by, created_at) values (")
                .bind(revision.organization_id.as_uuid())
                .append(", ")
                .bind(revision.project_id.as_uuid())
                .append(", ")
                .bind(revision.environment_id.as_uuid())
                .append(", ")
                .bind(revision.application_id.as_uuid())
                .append(", ")
                .bind(revision.id.as_uuid())
                .append(", ")
                .bind(revision.revision_number)
                .append(", ")
                .bind(revision.parent_revision_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(
                    revision
                        .parent_definition_digest
                        .as_ref()
                        .map(Sha256Digest::as_str),
                )
                .append(", ")
                .bind(DURABLE_CELL_APPLICATION_SCHEMA)
                .append(", ")
                .bind(revision.definition.canonical_acl())
                .append(", ")
                .bind(revision.definition.digest().as_str())
                .append(", ")
                .bind(revision.definition.spec().build_run_id.as_uuid())
                .append(", ")
                .bind(revision.created_by.as_uuid())
                .append(", ")
                .bind(revision.created_at)
                .append(")"),
        )
        .await?,
    )
}

#[allow(clippy::too_many_arguments)]
async fn persist_write(
    transaction: &PostgresTransaction,
    record: &DurableCellApplicationRecord,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
    idempotency: &IdempotencyRequest,
) -> Result<(), PostgresPersistenceError> {
    store_outbox(transaction, event).await?;
    store_durable_cell_audit(transaction, record, actor_principal_id, action, request_id).await?;
    store_idempotency(
        transaction,
        idempotency,
        &DurableCellWriteReference::from(record),
    )
    .await
}

async fn store_durable_cell_audit(
    transaction: &PostgresTransaction,
    record: &DurableCellApplicationRecord,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    let projection =
        DurableCellProjectionIdentity::for_current_revision(&record.application, &record.revision)
            .map_err(PostgresPersistenceError::Invariant)?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: record.application.organization_id.as_uuid(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: record.application.id.as_uuid(),
            occurred_at: record.application.updated_at,
            request_id,
            details: serde_json::json!({
                "projectId": record.application.project_id,
                "environmentId": record.application.environment_id,
                "revisionId": record.revision.id,
                "revisionNumber": record.revision.revision_number,
                "definitionDigest": record.revision.definition.digest(),
                "desiredState": record.application.desired_state,
                "storageNamespaceId": projection.storage_namespace_id,
                "workloadId": projection.workload_id,
                "workloadRevisionId": projection.workload_revision_id,
                "deploymentId": projection.deployment_id,
                "operationId": projection.operation_id,
            }),
        },
    )
    .await
}

fn require_updated(resource: &str, rows: u64) -> Result<(), PostgresPersistenceError> {
    match rows {
        1 => Ok(()),
        0 => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(format!("{resource} used a stale aggregate version")),
        )),
        rows => Err(PostgresPersistenceError::Invariant(format!(
            "{resource} affected {rows} rows"
        ))),
    }
}

fn decode_application(
    row: DurableCellApplicationRow,
) -> Result<DurableCellApplication, RepositoryError> {
    DurableCellApplication {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        environment_id: EnvironmentId::from_uuid(row.environment_id),
        id: DurableCellApplicationId::from_uuid(row.id),
        name: ResourceName::parse(row.name).map_err(stored("Durable Cell application name"))?,
        desired_state: DurableCellApplicationDesiredState::parse(&row.desired_state)
            .map_err(stored("Durable Cell desired state"))?,
        current_revision_id: DurableCellApplicationRevisionId::from_uuid(row.current_revision_id),
        current_revision_number: row.current_revision_number,
        current_definition_digest: Sha256Digest::parse(row.current_definition_digest)
            .map_err(stored("Durable Cell definition digest"))?,
        aggregate_version: row.aggregate_version,
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
    .restore()
    .map_err(stored("Durable Cell application state"))
}

fn decode_revision(
    row: DurableCellRevisionRow,
) -> Result<DurableCellApplicationRevision, RepositoryError> {
    if row.definition_schema != DURABLE_CELL_APPLICATION_SCHEMA {
        return Err(RepositoryError::Storage(
            "stored Durable Cell definition schema is invalid".into(),
        ));
    }
    let definition =
        DurableCellApplicationDefinition::restore(&row.canonical_acl, &row.definition_digest)
            .map_err(stored("Durable Cell application definition"))?;
    if definition.spec().build_run_id.as_uuid() != row.build_run_id {
        return Err(RepositoryError::Storage(
            "stored Durable Cell BuildRun binding is invalid".into(),
        ));
    }
    DurableCellApplicationRevision {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        environment_id: EnvironmentId::from_uuid(row.environment_id),
        application_id: DurableCellApplicationId::from_uuid(row.application_id),
        id: DurableCellApplicationRevisionId::from_uuid(row.id),
        revision_number: row.revision_number,
        parent_revision_id: row
            .parent_revision_id
            .map(DurableCellApplicationRevisionId::from_uuid),
        parent_definition_digest: row
            .parent_definition_digest
            .map(Sha256Digest::parse)
            .transpose()
            .map_err(stored("Durable Cell parent digest"))?,
        definition,
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
    }
    .restore()
    .map_err(stored("Durable Cell application revision"))
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}
