use super::validation::{
    form_name_key, validate_initial_draft, validate_publication, validate_publication_record,
    validate_revision,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::forms::domain::{
    CreateFormDraftWrite, FormDocument, FormDraft, FormPublicationRecord, FormRelease,
    FormReleaseContent, FormReleaseSummary, IFormRepository, PublishFormReleaseWrite,
    ReviseFormDraftWrite,
};
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, Sha256Digest,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresFormRepository {
    executor: PostgresExecutor,
}

impl PostgresFormRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

struct FormDraftRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    name: String,
    description: String,
    canonical_document_json: String,
    draft_digest: String,
    aggregate_version: u64,
    created_by: Uuid,
    updated_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    latest_release_id: Option<Uuid>,
    joined_release_id: Option<Uuid>,
    latest_release_revision: Option<u64>,
    latest_source_draft_version: Option<u64>,
    latest_release_digest: Option<String>,
    latest_release_published_at: Option<DateTime<Utc>>,
}

impl FromRow for FormDraftRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            id: decode(row, 2)?,
            name: decode(row, 3)?,
            description: decode(row, 4)?,
            canonical_document_json: decode(row, 5)?,
            draft_digest: decode(row, 6)?,
            aggregate_version: decode(row, 7)?,
            created_by: decode(row, 8)?,
            updated_by: decode(row, 9)?,
            created_at: decode(row, 10)?,
            updated_at: decode(row, 11)?,
            latest_release_id: decode(row, 12)?,
            joined_release_id: decode(row, 13)?,
            latest_release_revision: decode(row, 14)?,
            latest_source_draft_version: decode(row, 15)?,
            latest_release_digest: decode(row, 16)?,
            latest_release_published_at: decode(row, 17)?,
        })
    }
}

struct FormReleaseRow {
    organization_id: Uuid,
    project_id: Uuid,
    form_id: Uuid,
    id: Uuid,
    revision: u64,
    source_draft_version: u64,
    name: String,
    description: String,
    normalized_document_json: String,
    form_plan_json: String,
    compiler_revision: String,
    schema_profile: String,
    content_digest: String,
    published_by: Uuid,
    published_at: DateTime<Utc>,
}

impl FromRow for FormReleaseRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            form_id: decode(row, 2)?,
            id: decode(row, 3)?,
            revision: decode(row, 4)?,
            source_draft_version: decode(row, 5)?,
            name: decode(row, 6)?,
            description: decode(row, 7)?,
            normalized_document_json: decode(row, 8)?,
            form_plan_json: decode(row, 9)?,
            compiler_revision: decode(row, 10)?,
            schema_profile: decode(row, 11)?,
            content_digest: decode(row, 12)?,
            published_by: decode(row, 13)?,
            published_at: decode(row, 14)?,
        })
    }
}

#[async_trait]
impl IFormRepository for PostgresFormRepository {
    async fn replay_draft_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<FormDraft>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let replay = idempotency_replay::<FormDraft>(transaction, &idempotency).await?;
                    if let Some(replay) = &replay {
                        replay.value.validate().map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "Form draft replay target is invalid: {error}"
                            ))
                        })?;
                    }
                    Ok(replay)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_publication(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<FormPublicationRecord>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let replay =
                        idempotency_replay::<FormPublicationRecord>(transaction, &idempotency)
                            .await?;
                    if let Some(replay) = &replay {
                        validate_publication_record(&replay.value)?;
                    }
                    Ok(replay)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_draft(
        &self,
        write: CreateFormDraftWrite,
    ) -> Result<IdempotentWrite<FormDraft>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<FormDraft>(transaction, &write.idempotency).await?
                    {
                        replay.value.validate().map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "Form draft replay target is invalid: {error}"
                            ))
                        })?;
                        return Ok(replay);
                    }
                    validate_initial_draft(&write.draft, write.actor_principal_id)?;
                    match insert_draft(transaction, &write.draft).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Form name is already in use in this project".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_form_audit(
                        transaction,
                        &write.draft,
                        None,
                        write.actor_principal_id,
                        "form.draft.created",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.draft).await?;
                    Ok(IdempotentWrite {
                        value: write.draft,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revise_draft(
        &self,
        write: ReviseFormDraftWrite,
    ) -> Result<IdempotentWrite<FormDraft>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<FormDraft>(transaction, &write.idempotency).await?
                    {
                        replay.value.validate().map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "Form draft replay target is invalid: {error}"
                            ))
                        })?;
                        return Ok(replay);
                    }
                    let current =
                        lock_draft(transaction, write.draft.organization_id, write.draft.id)
                            .await?
                            .ok_or(RepositoryError::NotFound)?;
                    validate_revision(
                        &current,
                        &write.draft,
                        write.expected_version,
                        write.actor_principal_id,
                    )?;
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update form_drafts set name = ")
                            .bind(write.draft.name.as_str())
                            .append(", name_key = ")
                            .bind(form_name_key(&write.draft.name))
                            .append(", description = ")
                            .bind(write.draft.description.as_str())
                            .append(", canonical_document_json = ")
                            .bind(write.draft.document.canonical_json())
                            .append(", draft_digest = ")
                            .bind(write.draft.document.digest().as_str())
                            .append(", aggregate_version = ")
                            .bind(write.draft.aggregate_version)
                            .append(", updated_by = ")
                            .bind(write.draft.updated_by.as_uuid())
                            .append(", updated_at = ")
                            .bind(write.draft.updated_at)
                            .append(" where organization_id = ")
                            .bind(write.draft.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.draft.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version),
                    )
                    .await;
                    match updated {
                        Ok(1) => {}
                        Ok(0) => return Err(stale_draft().into()),
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "revising Form draft affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Form name is already in use in this project".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_form_audit(
                        transaction,
                        &write.draft,
                        None,
                        write.actor_principal_id,
                        "form.draft.revised",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.draft).await?;
                    Ok(IdempotentWrite {
                        value: write.draft,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn publish_release(
        &self,
        write: PublishFormReleaseWrite,
    ) -> Result<IdempotentWrite<FormPublicationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<FormPublicationRecord>(transaction, &write.idempotency)
                            .await?
                    {
                        validate_publication_record(&replay.value)?;
                        return Ok(replay);
                    }
                    let current = lock_draft(
                        transaction,
                        write.publication.draft.organization_id,
                        write.publication.draft.id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    validate_publication(
                        &current,
                        &write.publication,
                        write.expected_version,
                        write.actor_principal_id,
                    )?;
                    match insert_release(transaction, &write.publication.release).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Form draft version already has a published release".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    let draft = &write.publication.draft;
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update form_drafts set aggregate_version = ")
                            .bind(draft.aggregate_version)
                            .append(", latest_release_id = ")
                            .bind(write.publication.release.id.as_uuid())
                            .append(", updated_by = ")
                            .bind(draft.updated_by.as_uuid())
                            .append(", updated_at = ")
                            .bind(draft.updated_at)
                            .append(" where organization_id = ")
                            .bind(draft.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(draft.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version),
                    )
                    .await?;
                    if updated == 0 {
                        return Err(stale_draft().into());
                    }
                    if updated != 1 {
                        return Err(PostgresPersistenceError::Invariant(format!(
                            "publishing Form release affected {updated} draft rows"
                        )));
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_form_audit(
                        transaction,
                        draft,
                        Some(&write.publication.release),
                        write.actor_principal_id,
                        "form.release.published",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.publication).await?;
                    Ok(IdempotentWrite {
                        value: write.publication,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_draft(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
    ) -> Result<Option<FormDraft>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_optional::<FormDraftRow, _>(
                        transaction,
                        draft_select()
                            .append(" where draft.organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and draft.id = ")
                            .bind(form_id.as_uuid()),
                    )
                    .await?
                    .map(decode_draft)
                    .transpose()
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_drafts(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<FormDraft>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_all::<FormDraftRow, _>(
                        transaction,
                        draft_select()
                            .append(" where draft.organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and draft.project_id = ")
                            .bind(project_id.as_uuid())
                            .append(" order by draft.name_key asc, draft.id asc"),
                    )
                    .await?
                    .into_iter()
                    .map(decode_draft)
                    .collect()
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
        release_id: FormReleaseId,
    ) -> Result<Option<FormRelease>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_optional::<FormReleaseRow, _>(
                        transaction,
                        release_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and form_id = ")
                            .bind(form_id.as_uuid())
                            .append(" and id = ")
                            .bind(release_id.as_uuid()),
                    )
                    .await?
                    .map(decode_release)
                    .transpose()
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
    ) -> Result<Vec<FormRelease>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_all::<FormReleaseRow, _>(
                        transaction,
                        release_select()
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and form_id = ")
                            .bind(form_id.as_uuid())
                            .append(" order by revision desc, id asc"),
                    )
                    .await?
                    .into_iter()
                    .map(decode_release)
                    .collect()
                })
            })
            .await
            .map_err(transaction_error)
    }
}

fn draft_select() -> a3s_orm::SqlQuery<FormDraftRow> {
    sql_query::<FormDraftRow>(
        "select draft.organization_id, draft.project_id, draft.id, draft.name, draft.description, draft.canonical_document_json, draft.draft_digest, draft.aggregate_version, draft.created_by, draft.updated_by, draft.created_at, draft.updated_at, draft.latest_release_id, release.id, release.revision, release.source_draft_version, release.content_digest, release.published_at from form_drafts as draft left join form_releases as release on release.organization_id = draft.organization_id and release.form_id = draft.id and release.id = draft.latest_release_id",
    )
}

fn release_select() -> a3s_orm::SqlQuery<FormReleaseRow> {
    sql_query::<FormReleaseRow>(
        "select organization_id, project_id, form_id, id, revision, source_draft_version, name, description, normalized_document_json, form_plan_json, compiler_revision, schema_profile, content_digest, published_by, published_at from form_releases",
    )
}

async fn lock_draft(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    form_id: FormId,
) -> Result<Option<FormDraft>, PostgresPersistenceError> {
    fetch_optional::<FormDraftRow, _>(
        transaction,
        draft_select()
            .append(" where draft.organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and draft.id = ")
            .bind(form_id.as_uuid())
            .append(" for update of draft"),
    )
    .await?
    .map(decode_draft)
    .transpose()
}

async fn insert_draft(
    transaction: &a3s_orm::PostgresTransaction,
    draft: &FormDraft,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Form draft",
        execute(
            transaction,
            sql_query::<()>("insert into form_drafts (organization_id, project_id, id, name, name_key, description, canonical_document_json, draft_digest, aggregate_version, latest_release_id, created_by, updated_by, created_at, updated_at) values (")
                .bind(draft.organization_id.as_uuid())
                .append(", ")
                .bind(draft.project_id.as_uuid())
                .append(", ")
                .bind(draft.id.as_uuid())
                .append(", ")
                .bind(draft.name.as_str())
                .append(", ")
                .bind(form_name_key(&draft.name))
                .append(", ")
                .bind(draft.description.as_str())
                .append(", ")
                .bind(draft.document.canonical_json())
                .append(", ")
                .bind(draft.document.digest().as_str())
                .append(", ")
                .bind(draft.aggregate_version)
                .append(", ")
                .bind(draft.latest_release.as_ref().map(|release| release.id.as_uuid()))
                .append(", ")
                .bind(draft.created_by.as_uuid())
                .append(", ")
                .bind(draft.updated_by.as_uuid())
                .append(", ")
                .bind(draft.created_at)
                .append(", ")
                .bind(draft.updated_at)
                .append(")"),
        )
        .await?,
    )
}

async fn insert_release(
    transaction: &a3s_orm::PostgresTransaction,
    release: &FormRelease,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Form release",
        execute(
            transaction,
            sql_query::<()>("insert into form_releases (organization_id, project_id, form_id, id, revision, source_draft_version, name, description, normalized_document_json, form_plan_json, compiler_revision, schema_profile, content_digest, published_by, published_at) values (")
                .bind(release.organization_id.as_uuid())
                .append(", ")
                .bind(release.project_id.as_uuid())
                .append(", ")
                .bind(release.form_id.as_uuid())
                .append(", ")
                .bind(release.id.as_uuid())
                .append(", ")
                .bind(release.revision)
                .append(", ")
                .bind(release.source_draft_version)
                .append(", ")
                .bind(release.name.as_str())
                .append(", ")
                .bind(release.description.as_str())
                .append(", ")
                .bind(release.content.normalized_document_json())
                .append(", ")
                .bind(release.content.form_plan_json())
                .append(", ")
                .bind(release.content.compiler_revision())
                .append(", ")
                .bind(release.content.schema_profile())
                .append(", ")
                .bind(release.content.digest().as_str())
                .append(", ")
                .bind(release.published_by.as_uuid())
                .append(", ")
                .bind(release.published_at)
                .append(")"),
        )
        .await?,
    )
}

async fn store_form_audit(
    transaction: &a3s_orm::PostgresTransaction,
    draft: &FormDraft,
    release: Option<&FormRelease>,
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
            aggregate_id: draft.id.as_uuid(),
            occurred_at: draft.updated_at,
            request_id,
            scope: AuditWrite::resource_scope(
                draft.organization_id.as_uuid(),
                draft.project_id,
                None,
            ),
            details: serde_json::json!({
                "projectId": draft.project_id,
                "draftDigest": draft.document.digest(),
                "aggregateVersion": draft.aggregate_version,
                "releaseId": release.map(|value| value.id),
                "releaseRevision": release.map(|value| value.revision),
                "sourceDraftVersion": release.map(|value| value.source_draft_version),
                "contentDigest": release.map(|value| value.content.digest()),
            }),
        },
    )
    .await
}

fn decode_draft(row: FormDraftRow) -> Result<FormDraft, PostgresPersistenceError> {
    let document =
        FormDocument::restore(row.canonical_document_json, &row.draft_digest).map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored Form draft document is invalid: {error}"
            ))
        })?;
    let latest_release = match (
        row.latest_release_id,
        row.joined_release_id,
        row.latest_release_revision,
        row.latest_source_draft_version,
        row.latest_release_digest,
        row.latest_release_published_at,
    ) {
        (None, None, None, None, None, None) => None,
        (
            Some(head_id),
            Some(release_id),
            Some(revision),
            Some(source_draft_version),
            Some(digest),
            Some(published_at),
        ) if head_id == release_id => Some(FormReleaseSummary {
            id: FormReleaseId::from_uuid(release_id),
            revision,
            source_draft_version,
            digest: Sha256Digest::parse(digest).map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "stored latest Form release digest is invalid: {error}"
                ))
            })?,
            published_at,
        }),
        _ => {
            return Err(PostgresPersistenceError::Invariant(
                "stored Form draft latest release join is inconsistent".into(),
            ))
        }
    };
    FormDraft::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        FormId::from_uuid(row.id),
        row.name,
        row.description,
        document,
        row.aggregate_version,
        latest_release,
        PrincipalId::from_uuid(row.created_by),
        PrincipalId::from_uuid(row.updated_by),
        row.created_at,
        row.updated_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored Form draft is invalid: {error}"))
    })
}

fn decode_release(row: FormReleaseRow) -> Result<FormRelease, PostgresPersistenceError> {
    let content = FormReleaseContent::restore(
        row.normalized_document_json,
        row.form_plan_json,
        row.compiler_revision,
        row.schema_profile,
        &row.content_digest,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored Form release content is invalid: {error}"
        ))
    })?;
    FormRelease::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        FormId::from_uuid(row.form_id),
        FormReleaseId::from_uuid(row.id),
        row.revision,
        row.source_draft_version,
        row.name,
        row.description,
        content,
        PrincipalId::from_uuid(row.published_by),
        row.published_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored Form release is invalid: {error}"))
    })
}

fn stale_draft() -> RepositoryError {
    RepositoryError::Conflict("Form draft was changed from a stale aggregate version".into())
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
