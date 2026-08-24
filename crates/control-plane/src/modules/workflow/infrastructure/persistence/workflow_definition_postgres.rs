use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::repositories::WorkflowDefinitionWriteReference;
use crate::modules::workflow::domain::{
    CreateWorkflowDefinitionWrite, IWorkflowDefinitionRepository, ReviseWorkflowDefinitionWrite,
    WorkflowContract, WorkflowDefinition, WorkflowDefinitionRecord, WorkflowPayload,
    WorkflowPayloadKind, WorkflowRevision, WorkflowRevisionSemanticContractKind,
    WorkflowRevisionSemanticContracts,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresWorkflowDefinitionRepository {
    executor: PostgresExecutor,
}

impl PostgresWorkflowDefinitionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

struct WorkflowDefinitionRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    name: String,
    description: String,
    current_revision_id: Uuid,
    current_revision_number: u64,
    current_revision_digest: String,
    aggregate_version: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for WorkflowDefinitionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            id: decode(row, 2)?,
            name: decode(row, 3)?,
            description: decode(row, 4)?,
            current_revision_id: decode(row, 5)?,
            current_revision_number: decode(row, 6)?,
            current_revision_digest: decode(row, 7)?,
            aggregate_version: decode(row, 8)?,
            created_by: decode(row, 9)?,
            created_at: decode(row, 10)?,
            updated_at: decode(row, 11)?,
        })
    }
}

struct WorkflowRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    workflow_definition_id: Uuid,
    id: Uuid,
    revision_number: u64,
    parent_revision_id: Option<Uuid>,
    parent_digest: Option<String>,
    canonical_acl: String,
    content_digest: String,
    payload_set_digest: String,
    compiler_schema_version: u32,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for WorkflowRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            workflow_definition_id: decode(row, 2)?,
            id: decode(row, 3)?,
            revision_number: decode(row, 4)?,
            parent_revision_id: decode(row, 5)?,
            parent_digest: decode(row, 6)?,
            canonical_acl: decode(row, 7)?,
            content_digest: decode(row, 8)?,
            payload_set_digest: decode(row, 9)?,
            compiler_schema_version: decode(row, 10)?,
            created_by: decode(row, 11)?,
            created_at: decode(row, 12)?,
        })
    }
}

struct WorkflowPayloadRow {
    kind: String,
    canonical_acl: String,
    digest: String,
}

struct WorkflowSemanticContractRow {
    kind: String,
    canonical_acl: String,
    digest: String,
}

impl FromRow for WorkflowSemanticContractRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: decode(row, 0)?,
            canonical_acl: decode(row, 1)?,
            digest: decode(row, 2)?,
        })
    }
}

impl FromRow for WorkflowPayloadRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: decode(row, 0)?,
            canonical_acl: decode(row, 1)?,
            digest: decode(row, 2)?,
        })
    }
}

#[async_trait]
impl IWorkflowDefinitionRepository for PostgresWorkflowDefinitionRepository {
    async fn create(
        &self,
        write: CreateWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<WorkflowDefinitionWriteReference>(
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
                    validate_record(&write.record)?;
                    if write.record.revision.revision_number != 1
                        || write.record.revision.parent_revision_id.is_some()
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "initial WorkflowDefinition write contains a non-initial revision"
                                .into(),
                        ));
                    }
                    let insertion = async {
                        insert_definition(transaction, &write.record.definition).await?;
                        insert_revision(transaction, &write.record.revision).await
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Workflow name is already in use in this project".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_workflow_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        "workflow.definition.created",
                        write.request_id,
                    )
                    .await?;
                    let reference = WorkflowDefinitionWriteReference {
                        organization_id: write.record.definition.organization_id,
                        workflow_definition_id: write.record.definition.id,
                        workflow_revision_id: write.record.revision.id,
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

    async fn revise(
        &self,
        write: ReviseWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<WorkflowDefinitionWriteReference>(
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
                    validate_record(&write.record)?;
                    let current = fetch_optional::<WorkflowDefinitionRow, _>(
                        transaction,
                        definition_select()
                            .append(" where organization_id = ")
                            .bind(write.record.definition.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.definition.id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .map(decode_definition)
                    .transpose()?
                    .ok_or(RepositoryError::NotFound)?;
                    validate_successor(&current, &write)?;
                    match insert_revision(transaction, &write.record.revision).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Workflow revision already exists".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update workflow_definitions set name = ")
                            .bind(write.record.definition.name.as_str())
                            .append(", name_key = ")
                            .bind(workflow_name_key(&write.record.definition.name))
                            .append(", description = ")
                            .bind(write.record.definition.description.as_str())
                            .append(", current_revision_id = ")
                            .bind(write.record.definition.current_revision_id.as_uuid())
                            .append(", current_revision_number = ")
                            .bind(write.record.definition.current_revision_number)
                            .append(", current_revision_digest = ")
                            .bind(write.record.definition.current_revision_digest.as_str())
                            .append(", aggregate_version = ")
                            .bind(write.record.definition.aggregate_version)
                            .append(", updated_at = ")
                            .bind(write.record.definition.updated_at)
                            .append(" where organization_id = ")
                            .bind(write.record.definition.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.definition.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version),
                    )
                    .await;
                    match updated {
                        Ok(1) => {}
                        Ok(0) => {
                            return Err(RepositoryError::Conflict(
                                "WorkflowDefinition was revised from a stale aggregate version"
                                    .into(),
                            )
                            .into())
                        }
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "revising WorkflowDefinition affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Workflow name is already in use in this project".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_workflow_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        "workflow.definition.revised",
                        write.request_id,
                    )
                    .await?;
                    let reference = WorkflowDefinitionWriteReference {
                        organization_id: write.record.definition.organization_id,
                        workflow_definition_id: write.record.definition.id,
                        workflow_revision_id: write.record.revision.id,
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
    ) -> Result<Option<WorkflowDefinitionRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(replay) = idempotency_replay::<WorkflowDefinitionWriteReference>(
                        transaction,
                        &idempotency,
                    )
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
        definition_id: WorkflowDefinitionId,
    ) -> Result<Option<WorkflowDefinition>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_optional::<WorkflowDefinitionRow, _>(
                        transaction,
                        definition_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(definition_id.as_uuid()),
                    )
                    .await?
                    .map(decode_definition)
                    .transpose()
                    .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowDefinition>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_all::<WorkflowDefinitionRow, _>(
                        transaction,
                        definition_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(project_id.as_uuid())
                            .append(" order by name_key asc, id asc"),
                    )
                    .await?
                    .into_iter()
                    .map(decode_definition)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
        revision_id: WorkflowRevisionId,
    ) -> Result<Option<WorkflowRevision>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let row = fetch_optional::<WorkflowRevisionRow, _>(
                        transaction,
                        revision_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and workflow_definition_id = ")
                            .bind(definition_id.as_uuid())
                            .append(" and id = ")
                            .bind(revision_id.as_uuid()),
                    )
                    .await?;
                    match row {
                        Some(row) => decode_revision(transaction, row).await.map(Some),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
    ) -> Result<Vec<WorkflowRevision>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<WorkflowRevisionRow, _>(
                        transaction,
                        revision_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and workflow_definition_id = ")
                            .bind(definition_id.as_uuid())
                            .append(" order by revision_number desc, id asc"),
                    )
                    .await?;
                    let mut values = Vec::with_capacity(rows.len());
                    for row in rows {
                        values.push(decode_revision(transaction, row).await?);
                    }
                    Ok(values)
                })
            })
            .await
            .map_err(transaction_error)
    }
}

fn definition_select() -> a3s_orm::SqlQuery<WorkflowDefinitionRow> {
    sql_query::<WorkflowDefinitionRow>(
        "select organization_id, project_id, id, name, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at from workflow_definitions",
    )
}

fn revision_select() -> a3s_orm::SqlQuery<WorkflowRevisionRow> {
    sql_query::<WorkflowRevisionRow>(
        "select organization_id, project_id, workflow_definition_id, id, revision_number, parent_revision_id, parent_digest, canonical_acl, content_digest, payload_set_digest, compiler_schema_version, created_by, created_at from workflow_revisions",
    )
}

async fn load_record(
    transaction: &a3s_orm::PostgresTransaction,
    reference: WorkflowDefinitionWriteReference,
) -> Result<WorkflowDefinitionRecord, PostgresPersistenceError> {
    let head = fetch_optional::<WorkflowDefinitionRow, _>(
        transaction,
        definition_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and id = ")
            .bind(reference.workflow_definition_id.as_uuid()),
    )
    .await?
    .map(decode_definition)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("WorkflowDefinition replay target is missing".into())
    })?;
    let row = fetch_optional::<WorkflowRevisionRow, _>(
        transaction,
        revision_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and workflow_definition_id = ")
            .bind(reference.workflow_definition_id.as_uuid())
            .append(" and id = ")
            .bind(reference.workflow_revision_id.as_uuid()),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Workflow revision replay target is missing".into())
    })?;
    let revision = decode_revision(transaction, row).await?;
    let definition = head.at_revision(&revision).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "WorkflowDefinition replay target is invalid: {error}"
        ))
    })?;
    Ok(WorkflowDefinitionRecord {
        definition,
        revision,
    })
}

async fn insert_definition(
    transaction: &a3s_orm::PostgresTransaction,
    definition: &WorkflowDefinition,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "WorkflowDefinition",
        execute(
            transaction,
            sql_query::<()>("insert into workflow_definitions (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                .bind(definition.organization_id.as_uuid())
                .append(", ")
                .bind(definition.project_id.as_uuid())
                .append(", ")
                .bind(definition.id.as_uuid())
                .append(", ")
                .bind(definition.name.as_str())
                .append(", ")
                .bind(workflow_name_key(&definition.name))
                .append(", ")
                .bind(definition.description.as_str())
                .append(", ")
                .bind(definition.current_revision_id.as_uuid())
                .append(", ")
                .bind(definition.current_revision_number)
                .append(", ")
                .bind(definition.current_revision_digest.as_str())
                .append(", ")
                .bind(definition.aggregate_version)
                .append(", ")
                .bind(definition.created_by.as_uuid())
                .append(", ")
                .bind(definition.created_at)
                .append(", ")
                .bind(definition.updated_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_revision(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &WorkflowRevision,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Workflow revision",
        execute(
            transaction,
            sql_query::<()>("insert into workflow_revisions (organization_id, project_id, workflow_definition_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, payload_set_digest, created_by, created_at) values (")
                .bind(revision.organization_id.as_uuid())
                .append(", ")
                .bind(revision.project_id.as_uuid())
                .append(", ")
                .bind(revision.workflow_definition_id.as_uuid())
                .append(", ")
                .bind(revision.id.as_uuid())
                .append(", ")
                .bind(revision.revision_number)
                .append(", ")
                .bind(revision.parent_revision_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(revision.parent_digest.as_ref().map(|digest| digest.as_str().to_owned()))
                .append(", ")
                .bind(revision.contract_schema())
                .append(", ")
                .bind(revision.compiler_schema_version)
                .append(", ")
                .bind(revision.contract.canonical_acl())
                .append(", ")
                .bind(revision.contract.digest().as_str())
                .append(", ")
                .bind(revision.payload_set_digest.as_str())
                .append(", ")
                .bind(revision.created_by.as_uuid())
                .append(", ")
                .bind(revision.created_at)
                .append(")"),
        )
        .await?,
    )?;
    for payload in &revision.payloads {
        require_one_row(
            "Workflow revision payload",
            execute(
                transaction,
                sql_query::<()>("insert into workflow_revision_payloads (organization_id, project_id, workflow_definition_id, workflow_revision_id, digest, kind, schema, canonical_acl) values (")
                    .bind(revision.organization_id.as_uuid())
                    .append(", ")
                    .bind(revision.project_id.as_uuid())
                    .append(", ")
                    .bind(revision.workflow_definition_id.as_uuid())
                    .append(", ")
                    .bind(revision.id.as_uuid())
                    .append(", ")
                    .bind(payload.digest().as_str())
                    .append(", ")
                    .bind(payload.kind().as_str())
                    .append(", ")
                    .bind(payload.schema())
                    .append(", ")
                    .bind(payload.canonical_acl())
                    .append(")"),
            )
            .await?,
        )?;
    }
    if let Some(contracts) = &revision.semantic_contracts {
        for contract in contracts.persisted_contracts() {
            require_one_row(
                "Workflow revision semantic contract",
                execute(
                    transaction,
                    sql_query::<()>("insert into workflow_revision_semantic_contracts (organization_id, project_id, workflow_definition_id, workflow_revision_id, kind, schema, canonical_acl, digest) values (")
                        .bind(revision.organization_id.as_uuid())
                        .append(", ")
                        .bind(revision.project_id.as_uuid())
                        .append(", ")
                        .bind(revision.workflow_definition_id.as_uuid())
                        .append(", ")
                        .bind(revision.id.as_uuid())
                        .append(", ")
                        .bind(contract.kind.as_str())
                        .append(", ")
                        .bind(contract.schema)
                        .append(", ")
                        .bind(contract.canonical_acl)
                        .append(", ")
                        .bind(contract.digest.as_str())
                        .append(")"),
                )
                .await?,
            )?;
        }
    }
    Ok(())
}

async fn store_workflow_audit(
    transaction: &a3s_orm::PostgresTransaction,
    record: &WorkflowDefinitionRecord,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: record.definition.organization_id.as_uuid(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: record.definition.id.as_uuid(),
            occurred_at: record.revision.created_at,
            request_id,
            attribution_scope: AuditWrite::project_attribution(record.definition.project_id, None),
            details: serde_json::json!({
                "projectId": record.definition.project_id,
                "revisionId": record.revision.id,
                "revisionNumber": record.revision.revision_number,
                "contentDigest": record.revision.contract.digest(),
                "payloadSetDigest": record.revision.payload_set_digest,
                "semanticContractSetDigest": record.revision.semantic_contract_set_digest(),
                "aggregateVersion": record.definition.aggregate_version,
            }),
        },
    )
    .await
}

fn validate_record(record: &WorkflowDefinitionRecord) -> Result<(), PostgresPersistenceError> {
    record
        .definition
        .validate()
        .and_then(|()| record.revision.validate())
        .map_err(PostgresPersistenceError::Invariant)?;
    if record.revision.organization_id != record.definition.organization_id
        || record.revision.project_id != record.definition.project_id
        || record.revision.workflow_definition_id != record.definition.id
        || record.revision.id != record.definition.current_revision_id
        || record.revision.revision_number != record.definition.current_revision_number
        || record.revision.contract.digest() != &record.definition.current_revision_digest
        || record.revision.contract.spec().name != record.definition.name
        || record.revision.contract.spec().description != record.definition.description
    {
        return Err(PostgresPersistenceError::Invariant(
            "WorkflowDefinition aggregate and current revision do not match".into(),
        ));
    }
    Ok(())
}

fn validate_successor(
    current: &WorkflowDefinition,
    write: &ReviseWorkflowDefinitionWrite,
) -> Result<(), PostgresPersistenceError> {
    let next = &write.record.definition;
    let revision = &write.record.revision;
    if current.aggregate_version != write.expected_version
        || next.aggregate_version != write.expected_version.saturating_add(1)
        || next.organization_id != current.organization_id
        || next.project_id != current.project_id
        || next.id != current.id
        || next.created_by != current.created_by
        || next.created_at != current.created_at
        || revision.parent_revision_id != Some(current.current_revision_id)
        || revision.parent_digest.as_ref() != Some(&current.current_revision_digest)
    {
        return Err(RepositoryError::Conflict(
            "WorkflowDefinition was revised from a stale aggregate version".into(),
        )
        .into());
    }
    Ok(())
}

fn decode_definition(row: WorkflowDefinitionRow) -> Result<WorkflowDefinition, RepositoryError> {
    let value = WorkflowDefinition {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        id: WorkflowDefinitionId::from_uuid(row.id),
        name: row.name,
        description: row.description,
        current_revision_id: WorkflowRevisionId::from_uuid(row.current_revision_id),
        current_revision_number: row.current_revision_number,
        current_revision_digest: Sha256Digest::parse(row.current_revision_digest).map_err(
            |error| RepositoryError::Storage(format!("stored Workflow digest is invalid: {error}")),
        )?,
        aggregate_version: row.aggregate_version,
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    value.validate().map_err(RepositoryError::Storage)?;
    Ok(value)
}

async fn decode_revision(
    transaction: &a3s_orm::PostgresTransaction,
    row: WorkflowRevisionRow,
) -> Result<WorkflowRevision, PostgresPersistenceError> {
    let payload_rows = fetch_all::<WorkflowPayloadRow, _>(
        transaction,
        sql_query::<WorkflowPayloadRow>("select kind, canonical_acl, digest from workflow_revision_payloads where organization_id = ")
            .bind(row.organization_id)
            .append(" and workflow_definition_id = ")
            .bind(row.workflow_definition_id)
            .append(" and workflow_revision_id = ")
            .bind(row.id)
            .append(" order by digest asc"),
    )
    .await?;
    let payloads = payload_rows
        .into_iter()
        .map(|payload| {
            let kind = WorkflowPayloadKind::parse(&payload.kind).map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "stored Workflow payload kind is invalid: {error}"
                ))
            })?;
            WorkflowPayload::restore(kind, &payload.canonical_acl, &payload.digest).map_err(
                |error| {
                    PostgresPersistenceError::Invariant(format!(
                        "stored Workflow payload is invalid: {error}"
                    ))
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let parent_digest = row
        .parent_digest
        .as_deref()
        .map(Sha256Digest::parse)
        .transpose()
        .map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored Workflow parent digest is invalid: {error}"
            ))
        })?;
    let semantic_contracts = decode_semantic_contracts(transaction, &row).await?;
    WorkflowRevision::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        WorkflowDefinitionId::from_uuid(row.workflow_definition_id),
        WorkflowRevisionId::from_uuid(row.id),
        row.revision_number,
        row.parent_revision_id.map(WorkflowRevisionId::from_uuid),
        parent_digest,
        &row.canonical_acl,
        &row.content_digest,
        payloads,
        &row.payload_set_digest,
        semantic_contracts,
        row.compiler_schema_version,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored Workflow revision is invalid: {error}"))
    })
}

async fn decode_semantic_contracts(
    transaction: &a3s_orm::PostgresTransaction,
    row: &WorkflowRevisionRow,
) -> Result<Option<WorkflowRevisionSemanticContracts>, PostgresPersistenceError> {
    let rows = fetch_all::<WorkflowSemanticContractRow, _>(
        transaction,
        sql_query::<WorkflowSemanticContractRow>("select kind, canonical_acl, digest from workflow_revision_semantic_contracts where organization_id = ")
            .bind(row.organization_id)
            .append(" and workflow_definition_id = ")
            .bind(row.workflow_definition_id)
            .append(" and workflow_revision_id = ")
            .bind(row.id)
            .append(" order by kind asc"),
    )
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    if !matches!(rows.len(), 3..=5) {
        return Err(PostgresPersistenceError::Invariant(
            "stored Workflow revision semantic contract set is incomplete".into(),
        ));
    }
    let mut by_kind = std::collections::BTreeMap::new();
    for contract in &rows {
        let kind =
            WorkflowRevisionSemanticContractKind::parse(&contract.kind).map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "stored Workflow semantic contract kind is invalid: {error}"
                ))
            })?;
        if by_kind.insert(kind, contract).is_some() {
            return Err(PostgresPersistenceError::Invariant(
                "stored Workflow semantic contract kind is duplicated".into(),
            ));
        }
    }
    let require = |kind| {
        by_kind.get(&kind).copied().ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "stored Workflow revision semantic contract set is incomplete".into(),
            )
        })
    };
    let bindings = require(WorkflowRevisionSemanticContractKind::DescriptorBindings)?;
    let registry = require(WorkflowRevisionSemanticContractKind::DescriptorRegistry)?;
    let variables = require(WorkflowRevisionSemanticContractKind::VariableContract)?;
    let defaults = by_kind
        .get(&WorkflowRevisionSemanticContractKind::VariableDefaults)
        .copied();
    let composite_regions = by_kind
        .get(&WorkflowRevisionSemanticContractKind::CompositeRegions)
        .copied();
    let workflow =
        WorkflowContract::restore(&row.canonical_acl, &row.content_digest).map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored Workflow contract is invalid: {error}"
            ))
        })?;
    WorkflowRevisionSemanticContracts::restore_with_optional_contracts(
        workflow.spec(),
        &bindings.canonical_acl,
        &bindings.digest,
        &registry.canonical_acl,
        &registry.digest,
        &variables.canonical_acl,
        &variables.digest,
        defaults.map(|value| (value.canonical_acl.as_str(), value.digest.as_str())),
        composite_regions.map(|value| (value.canonical_acl.as_str(), value.digest.as_str())),
    )
    .map(Some)
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored Workflow semantic contracts are invalid: {error}"
        ))
    })
}

fn workflow_name_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
