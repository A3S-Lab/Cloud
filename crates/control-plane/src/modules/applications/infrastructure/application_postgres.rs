use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::applications::domain::{
    Application, ApplicationExperience, ApplicationRecord, ApplicationRelease,
    ApplicationWriteReference, CreateApplicationWrite, IApplicationRepository,
    PublishApplicationReleaseWrite, APPLICATION_RELEASE_CONTRACT_SCHEMA,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, IdempotencyRequest, IdempotentWrite, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, ResourceName, Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_APPLICATIONS: &str = "select organization_id, project_id, id, name, description, experience, current_release_id, current_release_number, current_release_digest, aggregate_version, created_by, created_at, updated_at from applications";
const SELECT_RELEASES: &str = "select organization_id, project_id, application_id, id, release_number, parent_release_id, parent_digest, contract_schema, canonical_acl, contract_digest, experience, workflow_definition_id, workflow_revision_id, workflow_contract_digest, workflow_payload_set_digest, workflow_semantic_contract_set_digest, input_schema_digest, output_schema_digest, presentation_digest, created_by, created_at from application_releases";

#[derive(Clone)]
pub struct PostgresApplicationRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationRepository for PostgresApplicationRepository {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ApplicationRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) =
                        idempotency_replay::<ApplicationWriteReference>(transaction, &idempotency)
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
        write: CreateApplicationWrite,
    ) -> Result<IdempotentWrite<ApplicationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<ApplicationWriteReference>(
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
                        insert_release(transaction, &write.record.release).await
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Application name or release identity is already in use".into(),
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
                        "application.release.created",
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

    async fn publish_release(
        &self,
        write: PublishApplicationReleaseWrite,
    ) -> Result<IdempotentWrite<ApplicationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<ApplicationWriteReference>(
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
                        write.record.application.id,
                    )
                    .await?;
                    write.validate_against(&current).map_err(|error| {
                        PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                    })?;
                    match insert_release(transaction, &write.record.release).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Application release identity is already in use".into(),
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
                        sql_query::<()>("update applications set current_release_id = ")
                            .bind(write.record.application.current_release_id.as_uuid())
                            .append(", current_release_number = ")
                            .bind(write.record.application.current_release_number)
                            .append(", current_release_digest = ")
                            .bind(write.record.application.current_release_digest.as_str())
                            .append(", aggregate_version = ")
                            .bind(write.record.application.aggregate_version)
                            .append(", updated_at = ")
                            .bind(write.record.application.updated_at)
                            .append(" where organization_id = ")
                            .bind(write.record.application.organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(write.record.application.project_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.application.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and current_release_id = ")
                            .bind(current.current_release_id.as_uuid()),
                    )
                    .await?;
                    require_updated("publishing Application release", updated)?;
                    persist_write(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        "application.release.published",
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
        application_id: ApplicationId,
    ) -> Result<Option<Application>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                application_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
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
        limit: usize,
    ) -> Result<Vec<Application>, RepositoryError> {
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

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        release_id: ApplicationReleaseId,
    ) -> Result<Option<ApplicationRelease>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                release_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" and id = ")
                    .bind(release_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_release)
            .transpose()
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        limit: usize,
    ) -> Result<Vec<ApplicationRelease>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                release_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" order by release_number desc, id asc limit ")
                    .bind(limit),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(decode_release)
            .collect()
    }
}

struct ApplicationRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    name: String,
    description: String,
    experience: String,
    current_release_id: Uuid,
    current_release_number: u64,
    current_release_digest: String,
    aggregate_version: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for ApplicationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            id: decode(row, 2)?,
            name: decode(row, 3)?,
            description: decode(row, 4)?,
            experience: decode(row, 5)?,
            current_release_id: decode(row, 6)?,
            current_release_number: decode(row, 7)?,
            current_release_digest: decode(row, 8)?,
            aggregate_version: decode(row, 9)?,
            created_by: decode(row, 10)?,
            created_at: decode(row, 11)?,
            updated_at: decode(row, 12)?,
        })
    }
}

struct ApplicationReleaseRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    id: Uuid,
    release_number: u64,
    parent_release_id: Option<Uuid>,
    parent_digest: Option<String>,
    contract_schema: String,
    canonical_acl: String,
    contract_digest: String,
    experience: String,
    workflow_definition_id: Uuid,
    workflow_revision_id: Uuid,
    workflow_contract_digest: String,
    workflow_payload_set_digest: String,
    workflow_semantic_contract_set_digest: String,
    input_schema_digest: String,
    output_schema_digest: String,
    presentation_digest: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for ApplicationReleaseRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            id: decode(row, 3)?,
            release_number: decode(row, 4)?,
            parent_release_id: decode(row, 5)?,
            parent_digest: decode(row, 6)?,
            contract_schema: decode(row, 7)?,
            canonical_acl: decode(row, 8)?,
            contract_digest: decode(row, 9)?,
            experience: decode(row, 10)?,
            workflow_definition_id: decode(row, 11)?,
            workflow_revision_id: decode(row, 12)?,
            workflow_contract_digest: decode(row, 13)?,
            workflow_payload_set_digest: decode(row, 14)?,
            workflow_semantic_contract_set_digest: decode(row, 15)?,
            input_schema_digest: decode(row, 16)?,
            output_schema_digest: decode(row, 17)?,
            presentation_digest: decode(row, 18)?,
            created_by: decode(row, 19)?,
            created_at: decode(row, 20)?,
        })
    }
}

fn application_select() -> a3s_orm::SqlQuery<ApplicationRow> {
    sql_query::<ApplicationRow>(SELECT_APPLICATIONS)
}

fn release_select() -> a3s_orm::SqlQuery<ApplicationReleaseRow> {
    sql_query::<ApplicationReleaseRow>(SELECT_RELEASES)
}

async fn lock_application(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
) -> Result<Application, PostgresPersistenceError> {
    fetch_optional::<ApplicationRow, _>(
        transaction,
        application_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and id = ")
            .bind(application_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_application)
    .transpose()?
    .ok_or_else(|| PostgresPersistenceError::Repository(RepositoryError::NotFound))
}

async fn fetch_release(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    release_id: ApplicationReleaseId,
) -> Result<Option<ApplicationRelease>, PostgresPersistenceError> {
    fetch_optional::<ApplicationReleaseRow, _>(
        transaction,
        release_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and application_id = ")
            .bind(application_id.as_uuid())
            .append(" and id = ")
            .bind(release_id.as_uuid()),
    )
    .await?
    .map(decode_release)
    .transpose()
    .map_err(Into::into)
}

async fn load_record(
    transaction: &PostgresTransaction,
    reference: ApplicationWriteReference,
) -> Result<ApplicationRecord, PostgresPersistenceError> {
    let head = fetch_optional::<ApplicationRow, _>(
        transaction,
        application_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and id = ")
            .bind(reference.application_id.as_uuid()),
    )
    .await?
    .map(decode_application)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Application replay head is missing".into())
    })?;
    let release = fetch_release(
        transaction,
        reference.organization_id,
        reference.project_id,
        reference.application_id,
        reference.release_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Application replay release is missing".into())
    })?;
    let application = head
        .at_release(&release)
        .map_err(PostgresPersistenceError::Invariant)?;
    ApplicationRecord::new(application, release).map_err(PostgresPersistenceError::Invariant)
}

async fn insert_application(
    transaction: &PostgresTransaction,
    application: &Application,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Application",
        execute(
            transaction,
            sql_query::<()>("insert into applications (organization_id, project_id, id, name, name_key, description, experience, current_release_id, current_release_number, current_release_digest, aggregate_version, created_by, created_at, updated_at) values (")
                .bind(application.organization_id.as_uuid())
                .append(", ")
                .bind(application.project_id.as_uuid())
                .append(", ")
                .bind(application.id.as_uuid())
                .append(", ")
                .bind(application.name.as_str())
                .append(", ")
                .bind(application.name.key())
                .append(", ")
                .bind(application.description.as_str())
                .append(", ")
                .bind(application.experience.as_str())
                .append(", ")
                .bind(application.current_release_id.as_uuid())
                .append(", ")
                .bind(application.current_release_number)
                .append(", ")
                .bind(application.current_release_digest.as_str())
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

async fn insert_release(
    transaction: &PostgresTransaction,
    release: &ApplicationRelease,
) -> Result<(), PostgresPersistenceError> {
    let workflow = &release.contract.spec().workflow;
    require_one_row(
        "Application release",
        execute(
            transaction,
            sql_query::<()>("insert into application_releases (organization_id, project_id, application_id, id, release_number, parent_release_id, parent_digest, contract_schema, canonical_acl, contract_digest, experience, workflow_definition_id, workflow_revision_id, workflow_contract_digest, workflow_payload_set_digest, workflow_semantic_contract_set_digest, input_schema_digest, output_schema_digest, presentation_digest, created_by, created_at) values (")
                .bind(release.organization_id.as_uuid())
                .append(", ")
                .bind(release.project_id.as_uuid())
                .append(", ")
                .bind(release.application_id.as_uuid())
                .append(", ")
                .bind(release.id.as_uuid())
                .append(", ")
                .bind(release.release_number)
                .append(", ")
                .bind(release.parent_release_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(release.parent_digest.as_ref().map(Sha256Digest::as_str))
                .append(", ")
                .bind(APPLICATION_RELEASE_CONTRACT_SCHEMA)
                .append(", ")
                .bind(release.contract.canonical_acl())
                .append(", ")
                .bind(release.contract.digest().as_str())
                .append(", ")
                .bind(release.contract.spec().experience.as_str())
                .append(", ")
                .bind(workflow.workflow_definition_id.as_uuid())
                .append(", ")
                .bind(workflow.workflow_revision_id.as_uuid())
                .append(", ")
                .bind(workflow.workflow_contract_digest.as_str())
                .append(", ")
                .bind(workflow.workflow_payload_set_digest.as_str())
                .append(", ")
                .bind(workflow.workflow_semantic_contract_set_digest.as_str())
                .append(", ")
                .bind(workflow.input_schema_digest.as_str())
                .append(", ")
                .bind(workflow.output_schema_digest.as_str())
                .append(", ")
                .bind(release.contract.spec().presentation_digest.as_str())
                .append(", ")
                .bind(release.created_by.as_uuid())
                .append(", ")
                .bind(release.created_at)
                .append(")"),
        )
        .await?,
    )
}

#[allow(clippy::too_many_arguments)]
async fn persist_write(
    transaction: &PostgresTransaction,
    record: &ApplicationRecord,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
    idempotency: &IdempotencyRequest,
) -> Result<(), PostgresPersistenceError> {
    store_outbox(transaction, event).await?;
    store_application_audit(transaction, record, actor_principal_id, action, request_id).await?;
    store_idempotency(
        transaction,
        idempotency,
        &ApplicationWriteReference::from(record),
    )
    .await
}

async fn store_application_audit(
    transaction: &PostgresTransaction,
    record: &ApplicationRecord,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    let workflow = &record.release.contract.spec().workflow;
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
                "applicationId": record.application.id,
                "releaseId": record.release.id,
                "releaseNumber": record.release.release_number,
                "experience": record.application.experience,
                "contractDigest": record.release.contract.digest(),
                "workflowDefinitionId": workflow.workflow_definition_id,
                "workflowRevisionId": workflow.workflow_revision_id,
                "workflowContractDigest": workflow.workflow_contract_digest,
                "workflowPayloadSetDigest": workflow.workflow_payload_set_digest,
                "workflowSemanticContractSetDigest": workflow.workflow_semantic_contract_set_digest,
                "inputSchemaDigest": workflow.input_schema_digest,
                "outputSchemaDigest": workflow.output_schema_digest,
                "presentationDigest": record.release.contract.spec().presentation_digest,
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

fn decode_application(row: ApplicationRow) -> Result<Application, RepositoryError> {
    let value = Application {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        id: ApplicationId::from_uuid(row.id),
        name: ResourceName::parse(row.name).map_err(stored("Application name"))?,
        description: row.description,
        experience: ApplicationExperience::parse(&row.experience)
            .map_err(stored("Application experience"))?,
        current_release_id: ApplicationReleaseId::from_uuid(row.current_release_id),
        current_release_number: row.current_release_number,
        current_release_digest: Sha256Digest::parse(row.current_release_digest)
            .map_err(stored("Application release digest"))?,
        aggregate_version: row.aggregate_version,
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    value.validate().map_err(stored("Application aggregate"))?;
    Ok(value)
}

fn decode_release(row: ApplicationReleaseRow) -> Result<ApplicationRelease, RepositoryError> {
    if row.contract_schema != APPLICATION_RELEASE_CONTRACT_SCHEMA {
        return Err(RepositoryError::Storage(
            "stored Application release schema is invalid".into(),
        ));
    }
    let release = ApplicationRelease::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        ApplicationId::from_uuid(row.application_id),
        ApplicationReleaseId::from_uuid(row.id),
        row.release_number,
        row.parent_release_id.map(ApplicationReleaseId::from_uuid),
        row.parent_digest
            .map(Sha256Digest::parse)
            .transpose()
            .map_err(stored("Application parent digest"))?,
        &row.canonical_acl,
        &row.contract_digest,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(stored("Application release"))?;
    let spec = release.contract.spec();
    let workflow = &spec.workflow;
    if row.experience != spec.experience.as_str()
        || row.workflow_definition_id != workflow.workflow_definition_id.as_uuid()
        || row.workflow_revision_id != workflow.workflow_revision_id.as_uuid()
        || row.workflow_contract_digest != workflow.workflow_contract_digest.as_str()
        || row.workflow_payload_set_digest != workflow.workflow_payload_set_digest.as_str()
        || row.workflow_semantic_contract_set_digest
            != workflow.workflow_semantic_contract_set_digest.as_str()
        || row.input_schema_digest != workflow.input_schema_digest.as_str()
        || row.output_schema_digest != workflow.output_schema_digest.as_str()
        || row.presentation_digest != spec.presentation_digest.as_str()
    {
        return Err(RepositoryError::Storage(
            "stored Application release evidence drifted from canonical ACL".into(),
        ));
    }
    Ok(release)
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

#[cfg(test)]
mod typed_orm_tests {
    #[test]
    fn repository_uses_the_shared_typed_a3s_orm_transaction_boundary() {
        let source = include_str!("application_postgres.rs");
        for required in [
            "PostgresExecutor",
            "sql_query::<",
            "idempotency_replay",
            "store_idempotency",
            "store_outbox",
            "store_audit",
        ] {
            assert!(
                source.contains(required),
                "missing shared persistence primitive {required}"
            );
        }
        for forbidden in [
            ["sqlx", "::"].concat(),
            ["tokio_", "postgres", "::"].concat(),
            ["create", "table"].join(" "),
            ["retry", "_count"].concat(),
        ] {
            assert!(
                !source.to_ascii_lowercase().contains(&forbidden),
                "Applications repository bypassed the shared persistence boundary: {forbidden}"
            );
        }
    }
}
