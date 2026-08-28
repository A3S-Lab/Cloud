use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::connectors::domain::{
    ConnectorProfile, ConnectorRecord, ConnectorRevision, ConnectorWriteReference,
    CreateConnectorProfileWrite, IConnectorProfileRepository, ReviseConnectorProfileWrite,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotentWrite, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, ResourceName, Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_PROFILES: &str = "select organization_id, project_id, environment_id, id, name, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at from connector_profiles";
const SELECT_REVISIONS: &str = "select organization_id, project_id, environment_id, profile_id, id, revision_number, parent_revision_id, parent_digest, definition_kind, definition_schema, canonical_acl, definition_digest, secret_binding_count, created_by, created_at from connector_revisions";

#[derive(Clone)]
pub struct PostgresConnectorProfileRepository {
    executor: PostgresExecutor,
}

impl PostgresConnectorProfileRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IConnectorProfileRepository for PostgresConnectorProfileRepository {
    async fn replay_write(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<ConnectorRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) =
                        idempotency_replay::<ConnectorWriteReference>(transaction, &idempotency)
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
        write: CreateConnectorProfileWrite,
    ) -> Result<IdempotentWrite<ConnectorRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<ConnectorWriteReference>(
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
                        insert_profile(transaction, &write.record.profile).await?;
                        insert_revision(transaction, &write.record.revision).await?;
                        insert_secret_bindings(transaction, &write.record.revision).await
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Connector profile name or revision identity is already in use"
                                    .into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_connector_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        "connector.profile.created",
                        write.request_id,
                    )
                    .await?;
                    let reference = ConnectorWriteReference::from(&write.record);
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
        write: ReviseConnectorProfileWrite,
    ) -> Result<IdempotentWrite<ConnectorRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<ConnectorWriteReference>(
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
                    let current = fetch_optional::<ConnectorProfileRow, _>(
                        transaction,
                        profile_select()
                            .append(" where organization_id = ")
                            .bind(write.record.profile.organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(write.record.profile.project_id.as_uuid())
                            .append(" and environment_id = ")
                            .bind(write.record.profile.environment_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.profile.id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .map(decode_profile)
                    .transpose()?
                    .ok_or(RepositoryError::NotFound)?;
                    write.validate_against(&current).map_err(|error| {
                        PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                    })?;
                    let insertion = async {
                        insert_revision(transaction, &write.record.revision).await?;
                        insert_secret_bindings(transaction, &write.record.revision).await
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Connector revision identity is already in use".into(),
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
                        sql_query::<()>("update connector_profiles set current_revision_id = ")
                            .bind(write.record.profile.current_revision_id.as_uuid())
                            .append(", current_revision_number = ")
                            .bind(write.record.profile.current_revision_number)
                            .append(", current_revision_digest = ")
                            .bind(write.record.profile.current_revision_digest.as_str())
                            .append(", aggregate_version = ")
                            .bind(write.record.profile.aggregate_version)
                            .append(", updated_at = ")
                            .bind(write.record.profile.updated_at)
                            .append(" where organization_id = ")
                            .bind(write.record.profile.organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(write.record.profile.project_id.as_uuid())
                            .append(" and environment_id = ")
                            .bind(write.record.profile.environment_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.profile.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version),
                    )
                    .await?;
                    match updated {
                        1 => {}
                        0 => {
                            return Err(RepositoryError::Conflict(
                                "Connector profile was revised from a stale aggregate version"
                                    .into(),
                            )
                            .into())
                        }
                        rows => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "revising Connector profile affected {rows} rows"
                            )))
                        }
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_connector_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        "connector.profile.revised",
                        write.request_id,
                    )
                    .await?;
                    let reference = ConnectorWriteReference::from(&write.record);
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
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
    ) -> Result<Option<ConnectorProfile>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                profile_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and id = ")
                    .bind(profile_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_profile)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<ConnectorProfile>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                profile_select()
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
            .map(decode_profile)
            .collect()
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
    ) -> Result<Option<ConnectorRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                revision_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and profile_id = ")
                    .bind(profile_id.as_uuid())
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
        profile_id: ConnectorProfileId,
        limit: usize,
    ) -> Result<Vec<ConnectorRevision>, RepositoryError> {
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
                    .append(" and profile_id = ")
                    .bind(profile_id.as_uuid())
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

struct ConnectorProfileRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    name: String,
    current_revision_id: Uuid,
    current_revision_number: u64,
    current_revision_digest: String,
    aggregate_version: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for ConnectorProfileRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            id: decode(row, 3)?,
            name: decode(row, 4)?,
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

struct ConnectorRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    id: Uuid,
    revision_number: u64,
    parent_revision_id: Option<Uuid>,
    parent_digest: Option<String>,
    definition_kind: String,
    definition_schema: String,
    canonical_acl: String,
    definition_digest: String,
    secret_binding_count: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for ConnectorRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            profile_id: decode(row, 3)?,
            id: decode(row, 4)?,
            revision_number: decode(row, 5)?,
            parent_revision_id: decode(row, 6)?,
            parent_digest: decode(row, 7)?,
            definition_kind: decode(row, 8)?,
            definition_schema: decode(row, 9)?,
            canonical_acl: decode(row, 10)?,
            definition_digest: decode(row, 11)?,
            secret_binding_count: decode(row, 12)?,
            created_by: decode(row, 13)?,
            created_at: decode(row, 14)?,
        })
    }
}

fn profile_select() -> a3s_orm::SqlQuery<ConnectorProfileRow> {
    sql_query::<ConnectorProfileRow>(SELECT_PROFILES)
}

fn revision_select() -> a3s_orm::SqlQuery<ConnectorRevisionRow> {
    sql_query::<ConnectorRevisionRow>(SELECT_REVISIONS)
}

async fn load_record(
    transaction: &PostgresTransaction,
    reference: ConnectorWriteReference,
) -> Result<ConnectorRecord, PostgresPersistenceError> {
    let head = fetch_optional::<ConnectorProfileRow, _>(
        transaction,
        profile_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(reference.environment_id.as_uuid())
            .append(" and id = ")
            .bind(reference.profile_id.as_uuid()),
    )
    .await?
    .map(decode_profile)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Connector replay profile is missing".into())
    })?;
    let revision = fetch_optional::<ConnectorRevisionRow, _>(
        transaction,
        revision_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(reference.environment_id.as_uuid())
            .append(" and profile_id = ")
            .bind(reference.profile_id.as_uuid())
            .append(" and id = ")
            .bind(reference.revision_id.as_uuid()),
    )
    .await?
    .map(decode_revision)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Connector replay revision is missing".into())
    })?;
    let profile = head.at_revision(&revision).map_err(|error| {
        PostgresPersistenceError::Invariant(format!("Connector replay target is invalid: {error}"))
    })?;
    ConnectorRecord::new(profile, revision).map_err(PostgresPersistenceError::Invariant)
}

async fn insert_profile(
    transaction: &PostgresTransaction,
    profile: &ConnectorProfile,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Connector profile",
        execute(
            transaction,
            sql_query::<()>("insert into connector_profiles (organization_id, project_id, environment_id, id, name, name_key, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                .bind(profile.organization_id.as_uuid())
                .append(", ")
                .bind(profile.project_id.as_uuid())
                .append(", ")
                .bind(profile.environment_id.as_uuid())
                .append(", ")
                .bind(profile.id.as_uuid())
                .append(", ")
                .bind(profile.name.as_str())
                .append(", ")
                .bind(profile.name.key())
                .append(", ")
                .bind(profile.current_revision_id.as_uuid())
                .append(", ")
                .bind(profile.current_revision_number)
                .append(", ")
                .bind(profile.current_revision_digest.as_str())
                .append(", ")
                .bind(profile.aggregate_version)
                .append(", ")
                .bind(profile.created_by.as_uuid())
                .append(", ")
                .bind(profile.created_at)
                .append(", ")
                .bind(profile.updated_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_revision(
    transaction: &PostgresTransaction,
    revision: &ConnectorRevision,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Connector revision",
        execute(
            transaction,
            sql_query::<()>("insert into connector_revisions (organization_id, project_id, environment_id, profile_id, id, revision_number, parent_revision_id, parent_digest, definition_kind, definition_schema, canonical_acl, definition_digest, secret_binding_count, created_by, created_at) values (")
                .bind(revision.organization_id.as_uuid())
                .append(", ")
                .bind(revision.project_id.as_uuid())
                .append(", ")
                .bind(revision.environment_id.as_uuid())
                .append(", ")
                .bind(revision.profile_id.as_uuid())
                .append(", ")
                .bind(revision.id.as_uuid())
                .append(", ")
                .bind(revision.revision_number)
                .append(", ")
                .bind(revision.parent_revision_id.map(|id| id.as_uuid()))
                .append(", ")
                .bind(revision.parent_digest.as_ref().map(Sha256Digest::as_str))
                .append(", ")
                .bind(revision.definition.kind())
                .append(", ")
                .bind(revision.definition.schema())
                .append(", ")
                .bind(revision.definition.canonical_acl())
                .append(", ")
                .bind(revision.definition.digest().as_str())
                .append(", ")
                .bind(revision.definition.secret_bindings().len())
                .append(", ")
                .bind(revision.created_by.as_uuid())
                .append(", ")
                .bind(revision.created_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_secret_bindings(
    transaction: &PostgresTransaction,
    revision: &ConnectorRevision,
) -> Result<(), PostgresPersistenceError> {
    let mut bindings = revision.definition.secret_bindings();
    // Migration 110 locks each exact active Secret/version pair during admission. A canonical
    // order prevents two revisions with reversed semantic purposes from forming a lock cycle.
    bindings.sort_by(|left, right| {
        left.reference
            .secret_id
            .cmp(&right.reference.secret_id)
            .then_with(|| left.reference.version.cmp(&right.reference.version))
            .then_with(|| left.purpose.as_str().cmp(right.purpose.as_str()))
    });
    for binding in bindings {
        require_one_row(
            "Connector Secret binding",
            execute(
                transaction,
                sql_query::<()>("insert into connector_revision_secret_bindings (organization_id, project_id, environment_id, profile_id, revision_id, purpose, secret_id, secret_version) values (")
                    .bind(revision.organization_id.as_uuid())
                    .append(", ")
                    .bind(revision.project_id.as_uuid())
                    .append(", ")
                    .bind(revision.environment_id.as_uuid())
                    .append(", ")
                    .bind(revision.profile_id.as_uuid())
                    .append(", ")
                    .bind(revision.id.as_uuid())
                    .append(", ")
                    .bind(binding.purpose.as_str())
                    .append(", ")
                    .bind(binding.reference.secret_id.as_uuid())
                    .append(", ")
                    .bind(binding.reference.version)
                    .append(")"),
            )
            .await?,
        )?;
    }
    Ok(())
}

async fn store_connector_audit(
    transaction: &PostgresTransaction,
    record: &ConnectorRecord,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: record.profile.id.as_uuid(),
            occurred_at: record.revision.created_at,
            request_id,
            scope: AuditWrite::resource_scope(
                record.profile.organization_id.as_uuid(),
                record.profile.project_id,
                Some(record.profile.environment_id),
            ),
            details: serde_json::json!({
                "projectId": record.profile.project_id,
                "environmentId": record.profile.environment_id,
                "revisionId": record.revision.id,
                "revisionNumber": record.revision.revision_number,
                "definitionKind": record.revision.definition.kind(),
                "definitionSchema": record.revision.definition.schema(),
                "definitionDigest": record.revision.definition.digest(),
                "secretBindingCount": record.secret_bindings().len(),
            }),
        },
    )
    .await
}

fn decode_profile(row: ConnectorProfileRow) -> Result<ConnectorProfile, RepositoryError> {
    let profile = ConnectorProfile {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        environment_id: EnvironmentId::from_uuid(row.environment_id),
        id: ConnectorProfileId::from_uuid(row.id),
        name: ResourceName::parse(row.name).map_err(stored("Connector profile name"))?,
        current_revision_id: ConnectorRevisionId::from_uuid(row.current_revision_id),
        current_revision_number: row.current_revision_number,
        current_revision_digest: Sha256Digest::parse(row.current_revision_digest)
            .map_err(stored("Connector profile digest"))?,
        aggregate_version: row.aggregate_version,
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    profile
        .validate()
        .map_err(stored("Connector profile state"))?;
    Ok(profile)
}

fn decode_revision(row: ConnectorRevisionRow) -> Result<ConnectorRevision, RepositoryError> {
    let parent_digest = row
        .parent_digest
        .map(Sha256Digest::parse)
        .transpose()
        .map_err(stored("Connector parent digest"))?;
    let revision = ConnectorRevision::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        EnvironmentId::from_uuid(row.environment_id),
        ConnectorProfileId::from_uuid(row.profile_id),
        ConnectorRevisionId::from_uuid(row.id),
        row.revision_number,
        row.parent_revision_id.map(ConnectorRevisionId::from_uuid),
        parent_digest,
        &row.definition_kind,
        &row.definition_schema,
        &row.canonical_acl,
        &row.definition_digest,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(stored("Connector revision"))?;
    if usize::try_from(row.secret_binding_count).ok()
        != Some(revision.definition.secret_bindings().len())
    {
        return Err(RepositoryError::Storage(
            "stored Connector Secret binding count is invalid".into(),
        ));
    }
    Ok(revision)
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
