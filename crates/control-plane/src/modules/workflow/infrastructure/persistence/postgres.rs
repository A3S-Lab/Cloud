use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest,
};
use crate::modules::workflow::domain::repositories::OntologyWriteReference;
use crate::modules::workflow::domain::{
    CreateOntologyWrite, IOntologyRepository, Ontology, OntologyMigrationPolicy, OntologyName,
    OntologyRecord, OntologyRevision, ReviseOntologyWrite,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresOntologyRepository {
    executor: PostgresExecutor,
}

impl PostgresOntologyRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

struct OntologyRow {
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

impl FromRow for OntologyRow {
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

struct OntologyRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    ontology_id: Uuid,
    id: Uuid,
    revision_number: u64,
    parent_revision_id: Option<Uuid>,
    parent_digest: Option<String>,
    canonical_acl: String,
    content_digest: String,
    compiler_schema_version: u32,
    migration_policy: String,
    migration_rule_id: Option<String>,
    migration_digest: Option<String>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for OntologyRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            ontology_id: decode(row, 2)?,
            id: decode(row, 3)?,
            revision_number: decode(row, 4)?,
            parent_revision_id: decode(row, 5)?,
            parent_digest: decode(row, 6)?,
            canonical_acl: decode(row, 7)?,
            content_digest: decode(row, 8)?,
            compiler_schema_version: decode(row, 9)?,
            migration_policy: decode(row, 10)?,
            migration_rule_id: decode(row, 11)?,
            migration_digest: decode(row, 12)?,
            created_by: decode(row, 13)?,
            created_at: decode(row, 14)?,
        })
    }
}

#[async_trait]
impl IOntologyRepository for PostgresOntologyRepository {
    async fn create(
        &self,
        write: CreateOntologyWrite,
    ) -> Result<IdempotentWrite<OntologyRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<OntologyWriteReference>(
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
                            "initial Ontology write contains a non-initial revision".into(),
                        ));
                    }
                    let insertion = async {
                        insert_ontology(transaction, &write.record.ontology).await?;
                        insert_revision(transaction, &write.record.revision).await
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Ontology name is already in use in this project".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_ontology_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        "workflow.ontology.created",
                        write.request_id,
                    )
                    .await?;
                    let reference = OntologyWriteReference {
                        organization_id: write.record.ontology.organization_id,
                        ontology_id: write.record.ontology.id,
                        revision_id: write.record.revision.id,
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
        write: ReviseOntologyWrite,
    ) -> Result<IdempotentWrite<OntologyRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<OntologyWriteReference>(
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
                    let current = fetch_optional::<OntologyRow, _>(
                        transaction,
                        ontology_select()
                            .append(" where organization_id = ")
                            .bind(write.record.ontology.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.ontology.id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .map(decode_ontology)
                    .transpose()?
                    .ok_or(RepositoryError::NotFound)?;
                    validate_successor(&current, &write)?;
                    let insertion = insert_revision(transaction, &write.record.revision).await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Ontology revision already exists".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update ontologies set name = ")
                            .bind(write.record.ontology.name.as_str())
                            .append(", name_key = ")
                            .bind(write.record.ontology.name.key())
                            .append(", description = ")
                            .bind(write.record.ontology.description.as_str())
                            .append(", current_revision_id = ")
                            .bind(write.record.ontology.current_revision_id.as_uuid())
                            .append(", current_revision_number = ")
                            .bind(write.record.ontology.current_revision_number)
                            .append(", current_revision_digest = ")
                            .bind(write.record.ontology.current_revision_digest.as_str())
                            .append(", aggregate_version = ")
                            .bind(write.record.ontology.aggregate_version)
                            .append(", updated_at = ")
                            .bind(write.record.ontology.updated_at)
                            .append(" where organization_id = ")
                            .bind(write.record.ontology.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.ontology.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version),
                    )
                    .await;
                    match updated {
                        Ok(1) => {}
                        Ok(0) => {
                            return Err(RepositoryError::Conflict(
                                "Ontology was revised from a stale aggregate version".into(),
                            )
                            .into())
                        }
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "revising Ontology affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Ontology name is already in use in this project".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_ontology_audit(
                        transaction,
                        &write.record,
                        write.actor_principal_id,
                        "workflow.ontology.revised",
                        write.request_id,
                    )
                    .await?;
                    let reference = OntologyWriteReference {
                        organization_id: write.record.ontology.organization_id,
                        ontology_id: write.record.ontology.id,
                        revision_id: write.record.revision.id,
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
        ontology_id: OntologyId,
    ) -> Result<Option<Ontology>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                ontology_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(ontology_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_ontology)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<Ontology>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                ontology_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" order by name_key asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_ontology)
            .collect()
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
        revision_id: OntologyRevisionId,
    ) -> Result<Option<OntologyRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                revision_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and ontology_id = ")
                    .bind(ontology_id.as_uuid())
                    .append(" and id = ")
                    .bind(revision_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_revision)
            .transpose()
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
    ) -> Result<Vec<OntologyRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                revision_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and ontology_id = ")
                    .bind(ontology_id.as_uuid())
                    .append(" order by revision_number desc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_revision)
            .collect()
    }
}

fn ontology_select() -> a3s_orm::SqlQuery<OntologyRow> {
    sql_query::<OntologyRow>(
        "select organization_id, project_id, id, name, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at from ontologies",
    )
}

fn revision_select() -> a3s_orm::SqlQuery<OntologyRevisionRow> {
    sql_query::<OntologyRevisionRow>(
        "select organization_id, project_id, ontology_id, id, revision_number, parent_revision_id, parent_digest, canonical_acl, content_digest, compiler_schema_version, migration_policy, migration_rule_id, migration_digest, created_by, created_at from ontology_revisions",
    )
}

async fn load_record(
    transaction: &a3s_orm::PostgresTransaction,
    reference: OntologyWriteReference,
) -> Result<OntologyRecord, PostgresPersistenceError> {
    let head = fetch_optional::<OntologyRow, _>(
        transaction,
        ontology_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and id = ")
            .bind(reference.ontology_id.as_uuid()),
    )
    .await?
    .map(decode_ontology)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Ontology replay target is missing".into())
    })?;
    let revision = fetch_optional::<OntologyRevisionRow, _>(
        transaction,
        revision_select()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and ontology_id = ")
            .bind(reference.ontology_id.as_uuid())
            .append(" and id = ")
            .bind(reference.revision_id.as_uuid()),
    )
    .await?
    .map(decode_revision)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Ontology revision replay target is missing".into())
    })?;
    let ontology = head.at_revision(&revision).map_err(|error| {
        PostgresPersistenceError::Invariant(format!("Ontology replay target is invalid: {error}"))
    })?;
    Ok(OntologyRecord { ontology, revision })
}

async fn insert_ontology(
    transaction: &a3s_orm::PostgresTransaction,
    ontology: &Ontology,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Ontology",
        execute(
            transaction,
            sql_query::<()>("insert into ontologies (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                .bind(ontology.organization_id.as_uuid())
                .append(", ")
                .bind(ontology.project_id.as_uuid())
                .append(", ")
                .bind(ontology.id.as_uuid())
                .append(", ")
                .bind(ontology.name.as_str())
                .append(", ")
                .bind(ontology.name.key())
                .append(", ")
                .bind(ontology.description.as_str())
                .append(", ")
                .bind(ontology.current_revision_id.as_uuid())
                .append(", ")
                .bind(ontology.current_revision_number)
                .append(", ")
                .bind(ontology.current_revision_digest.as_str())
                .append(", ")
                .bind(ontology.aggregate_version)
                .append(", ")
                .bind(ontology.created_by.as_uuid())
                .append(", ")
                .bind(ontology.created_at)
                .append(", ")
                .bind(ontology.updated_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_revision(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &OntologyRevision,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Ontology revision",
        execute(
            transaction,
            sql_query::<()>("insert into ontology_revisions (organization_id, project_id, ontology_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, migration_policy, migration_rule_id, migration_digest, created_by, created_at) values (")
                .bind(revision.organization_id.as_uuid())
                .append(", ")
                .bind(revision.project_id.as_uuid())
                .append(", ")
                .bind(revision.ontology_id.as_uuid())
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
                .bind(revision.migration_policy.kind())
                .append(", ")
                .bind(revision.migration_policy.rule_id().map(str::to_owned))
                .append(", ")
                .bind(
                    revision
                        .migration_policy
                        .expression_digest()
                        .map(|digest| digest.as_str().to_owned()),
                )
                .append(", ")
                .bind(revision.created_by.as_uuid())
                .append(", ")
                .bind(revision.created_at)
                .append(")"),
        )
        .await?,
    )
}

async fn store_ontology_audit(
    transaction: &a3s_orm::PostgresTransaction,
    record: &OntologyRecord,
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
            aggregate_id: record.ontology.id.as_uuid(),
            occurred_at: record.revision.created_at,
            request_id,
            scope: AuditWrite::resource_scope(
                record.ontology.organization_id.as_uuid(),
                record.ontology.project_id,
                None,
            ),
            details: serde_json::json!({
                "projectId": record.ontology.project_id,
                "revisionId": record.revision.id,
                "revisionNumber": record.revision.revision_number,
                "contentDigest": record.revision.contract.digest(),
                "migrationPolicy": record.revision.migration_policy.kind(),
                "migrationRuleId": record.revision.migration_policy.rule_id(),
                "aggregateVersion": record.ontology.aggregate_version,
            }),
        },
    )
    .await
}

fn validate_record(record: &OntologyRecord) -> Result<(), PostgresPersistenceError> {
    record
        .ontology
        .validate()
        .and_then(|()| record.revision.validate())
        .map_err(PostgresPersistenceError::Invariant)?;
    if record.revision.organization_id != record.ontology.organization_id
        || record.revision.project_id != record.ontology.project_id
        || record.revision.ontology_id != record.ontology.id
        || record.revision.id != record.ontology.current_revision_id
        || record.revision.revision_number != record.ontology.current_revision_number
        || record.revision.contract.digest() != &record.ontology.current_revision_digest
    {
        return Err(PostgresPersistenceError::Invariant(
            "Ontology aggregate and current revision do not match".into(),
        ));
    }
    Ok(())
}

fn validate_successor(
    current: &Ontology,
    write: &ReviseOntologyWrite,
) -> Result<(), PostgresPersistenceError> {
    let next = &write.record.ontology;
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
            "Ontology was revised from a stale aggregate version".into(),
        )
        .into());
    }
    Ok(())
}

fn decode_ontology(row: OntologyRow) -> Result<Ontology, RepositoryError> {
    let ontology = Ontology {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        id: OntologyId::from_uuid(row.id),
        name: OntologyName::parse(row.name).map_err(|error| {
            RepositoryError::Storage(format!("stored Ontology name is invalid: {error}"))
        })?,
        description: row.description,
        current_revision_id: OntologyRevisionId::from_uuid(row.current_revision_id),
        current_revision_number: row.current_revision_number,
        current_revision_digest: Sha256Digest::parse(row.current_revision_digest).map_err(
            |error| RepositoryError::Storage(format!("stored Ontology digest is invalid: {error}")),
        )?,
        aggregate_version: row.aggregate_version,
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    ontology.validate().map_err(RepositoryError::Storage)?;
    Ok(ontology)
}

fn decode_revision(row: OntologyRevisionRow) -> Result<OntologyRevision, RepositoryError> {
    let parent_digest = row
        .parent_digest
        .map(Sha256Digest::parse)
        .transpose()
        .map_err(|error| {
            RepositoryError::Storage(format!("stored parent digest is invalid: {error}"))
        })?;
    let migration_policy = match row.migration_policy.as_str() {
        "initial" if row.migration_rule_id.is_none() && row.migration_digest.is_none() => {
            OntologyMigrationPolicy::Initial
        }
        "compatible" if row.migration_rule_id.is_none() && row.migration_digest.is_none() => {
            OntologyMigrationPolicy::Compatible
        }
        "explicit" => OntologyMigrationPolicy::Explicit {
            rule_id: row.migration_rule_id.ok_or_else(|| {
                RepositoryError::Storage("stored explicit migration rule is missing".into())
            })?,
            expression_digest: Sha256Digest::parse(row.migration_digest.ok_or_else(|| {
                RepositoryError::Storage("stored explicit migration digest is missing".into())
            })?)
            .map_err(|error| {
                RepositoryError::Storage(format!("stored migration digest is invalid: {error}"))
            })?,
        },
        _ => {
            return Err(RepositoryError::Storage(
                "stored Ontology migration policy is invalid".into(),
            ))
        }
    };
    OntologyRevision::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        OntologyId::from_uuid(row.ontology_id),
        OntologyRevisionId::from_uuid(row.id),
        row.revision_number,
        row.parent_revision_id.map(OntologyRevisionId::from_uuid),
        parent_digest,
        &row.canonical_acl,
        &row.content_digest,
        row.compiler_schema_version,
        migration_policy,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(|error| {
        RepositoryError::Storage(format!("stored Ontology revision is invalid: {error}"))
    })
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
