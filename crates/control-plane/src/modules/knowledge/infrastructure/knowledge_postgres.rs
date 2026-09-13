use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::knowledge::domain::{
    AppendKnowledgeBaseRevision, AppendKnowledgeBaseWrite, CreateKnowledgeBase,
    CreateKnowledgeBaseWrite, CreateKnowledgePipeline, CreateKnowledgePipelineWrite,
    IKnowledgeBaseRepository, IKnowledgePipelineRepository, KnowledgeBaseRecord,
    KnowledgeBaseRevisionV1, KnowledgeBaseWriteReference, KnowledgePipelineRecord,
    KnowledgePipelineReleaseV1, KnowledgePipelineWriteReference, PublishKnowledgePipelineRelease,
    PublishKnowledgePipelineWrite,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, PrincipalId, RepositoryError,
};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_BASE_HEAD: &str = "select organization_id, project_id, knowledge_base_id, current_revision_id, current_generation, current_revision_digest, created_at, updated_at from knowledge_bases";
const SELECT_BASE_REVISION: &str = "select organization_id, knowledge_base_id, revision_id, generation, parent_revision_id, parent_digest, revision_digest, revision_acl from knowledge_base_revisions";
const SELECT_PIPELINE_HEAD: &str = "select organization_id, project_id, pipeline_id, current_release_id, current_release_digest, created_at, updated_at from knowledge_pipelines";
const SELECT_PIPELINE_RELEASE: &str = "select organization_id, pipeline_id, release_id, release_digest, release_acl from knowledge_pipeline_releases";

/// Durable KnowledgeBase revision catalog.
#[derive(Clone)]
pub struct PostgresKnowledgeBaseRepository {
    executor: PostgresExecutor,
}

impl PostgresKnowledgeBaseRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IKnowledgeBaseRepository for PostgresKnowledgeBaseRepository {
    async fn create(
        &self,
        request: CreateKnowledgeBase,
    ) -> Result<KnowledgeBaseRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let record = KnowledgeBaseRecord::new(request.revision, request.created_at)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    insert_base_head(transaction, &record).await?;
                    insert_base_revision(
                        transaction,
                        &record.revision,
                        None,
                        None,
                        record.created_at,
                    )
                    .await?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
    ) -> Result<Option<KnowledgeBaseRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_base_head(transaction, organization_id, knowledge_base_id, false).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list(&self, limit: usize) -> Result<Vec<KnowledgeBaseRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<KnowledgeBaseHeadRow, _>(
                        transaction,
                        sql_query::<KnowledgeBaseHeadRow>(SELECT_BASE_HEAD)
                            .append(
                                " order by organization_id asc, knowledge_base_id asc limit ",
                            )
                            .bind(limit),
                    )
                    .await?;
                    let mut records = Vec::with_capacity(rows.len());
                    for row in rows {
                        let Some(record) = load_base_head(
                            transaction,
                            row.organization_id,
                            row.knowledge_base_id,
                            false,
                        )
                        .await?
                        else {
                            return Err(PostgresPersistenceError::Invariant(
                                "KnowledgeBase head disappeared during discovery".into(),
                            ));
                        };
                        records.push(record);
                    }
                    Ok(records)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        revision_id: Uuid,
    ) -> Result<Option<KnowledgeBaseRevisionV1>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_base_revision(
                        transaction,
                        organization_id,
                        knowledge_base_id,
                        revision_id,
                        false,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn append_revision(
        &self,
        request: AppendKnowledgeBaseRevision,
    ) -> Result<KnowledgeBaseRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let current = load_base_head(
                        transaction,
                        request.organization_id,
                        request.knowledge_base_id,
                        true,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)
                    .map_err(PostgresPersistenceError::Repository)?;
                    let parent_revision_id = current.revision.spec().revision_id.as_uuid();
                    let parent_digest = current.revision.digest().as_str().to_string();
                    let updated = current
                        .append(
                            request.revision,
                            &request.expected_revision_digest,
                            request.updated_at,
                        )
                        .map_err(|error| {
                            PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                        })?;
                    insert_base_revision(
                        transaction,
                        &updated.revision,
                        Some(parent_revision_id),
                        Some(parent_digest.as_str()),
                        updated.updated_at,
                    )
                    .await?;
                    update_base_head(transaction, &updated).await?;
                    Ok(updated)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<KnowledgeBaseRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) = idempotency_replay::<KnowledgeBaseWriteReference>(
                        transaction,
                        &idempotency,
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    load_base_head(
                        transaction,
                        reference.value.organization_id.as_uuid(),
                        reference.value.knowledge_base_id,
                        false,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_write(
        &self,
        write: CreateKnowledgeBaseWrite,
    ) -> Result<IdempotentWrite<KnowledgeBaseRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<KnowledgeBaseWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        let record = load_base_head(
                            transaction,
                            reference.value.organization_id.as_uuid(),
                            reference.value.knowledge_base_id,
                            false,
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "KnowledgeBase idempotency reference is missing".into(),
                            )
                        })?;
                        return Ok(IdempotentWrite {
                            value: record,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    insert_base_head(transaction, &write.record).await?;
                    insert_base_revision(
                        transaction,
                        &write.record.revision,
                        None,
                        None,
                        write.record.created_at,
                    )
                    .await?;
                    persist_knowledge_base_side_effects(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                        "knowledge.base.created",
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

    async fn append_write(
        &self,
        write: AppendKnowledgeBaseWrite,
    ) -> Result<IdempotentWrite<KnowledgeBaseRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<KnowledgeBaseWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        let record = load_base_head(
                            transaction,
                            reference.value.organization_id.as_uuid(),
                            reference.value.knowledge_base_id,
                            false,
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "KnowledgeBase idempotency reference is missing".into(),
                            )
                        })?;
                        return Ok(IdempotentWrite {
                            value: record,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let organization_id = write.record.revision.spec().organization_id.as_uuid();
                    let knowledge_base_id =
                        write.record.revision.spec().knowledge_base_id.as_uuid();
                    let current = load_base_head(
                        transaction,
                        organization_id,
                        knowledge_base_id,
                        true,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)
                    .map_err(PostgresPersistenceError::Repository)?;
                    let parent_revision_id = current.revision.spec().revision_id.as_uuid();
                    let parent_digest = current.revision.digest().as_str().to_string();
                    let updated = current
                        .append(
                            write.record.revision.clone(),
                            &write.expected_revision_digest,
                            write.record.updated_at,
                        )
                        .map_err(|error| {
                            PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                        })?;
                    insert_base_revision(
                        transaction,
                        &updated.revision,
                        Some(parent_revision_id),
                        Some(parent_digest.as_str()),
                        updated.updated_at,
                    )
                    .await?;
                    update_base_head(transaction, &updated).await?;
                    persist_knowledge_base_side_effects(
                        transaction,
                        &updated,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                        "knowledge.base.revised",
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: updated,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

/// Durable KnowledgePipeline release catalog.
#[derive(Clone)]
pub struct PostgresKnowledgePipelineRepository {
    executor: PostgresExecutor,
}

impl PostgresKnowledgePipelineRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IKnowledgePipelineRepository for PostgresKnowledgePipelineRepository {
    async fn create(
        &self,
        request: CreateKnowledgePipeline,
    ) -> Result<KnowledgePipelineRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let record = KnowledgePipelineRecord::new(request.release, request.created_at)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    insert_pipeline_head(transaction, &record).await?;
                    insert_pipeline_release(transaction, &record.release, record.created_at).await?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        pipeline_id: Uuid,
    ) -> Result<Option<KnowledgePipelineRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_pipeline_head(transaction, organization_id, pipeline_id, false).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_release(
        &self,
        organization_id: Uuid,
        pipeline_id: Uuid,
        release_id: Uuid,
    ) -> Result<Option<KnowledgePipelineReleaseV1>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_pipeline_release(
                        transaction,
                        organization_id,
                        pipeline_id,
                        release_id,
                        false,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn publish_release(
        &self,
        request: PublishKnowledgePipelineRelease,
    ) -> Result<KnowledgePipelineRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let current = load_pipeline_head(
                        transaction,
                        request.organization_id,
                        request.pipeline_id,
                        true,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)
                    .map_err(PostgresPersistenceError::Repository)?;
                    let updated = current
                        .publish(
                            request.release,
                            &request.expected_release_digest,
                            request.updated_at,
                        )
                        .map_err(|error| {
                            PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                        })?;
                    insert_pipeline_release(transaction, &updated.release, updated.updated_at)
                        .await?;
                    update_pipeline_head(transaction, &updated).await?;
                    Ok(updated)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<KnowledgePipelineRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) = idempotency_replay::<KnowledgePipelineWriteReference>(
                        transaction,
                        &idempotency,
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    load_pipeline_head(
                        transaction,
                        reference.value.organization_id.as_uuid(),
                        reference.value.pipeline_id,
                        false,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_write(
        &self,
        write: CreateKnowledgePipelineWrite,
    ) -> Result<IdempotentWrite<KnowledgePipelineRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<KnowledgePipelineWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        let record = load_pipeline_head(
                            transaction,
                            reference.value.organization_id.as_uuid(),
                            reference.value.pipeline_id,
                            false,
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "KnowledgePipeline idempotency reference is missing".into(),
                            )
                        })?;
                        return Ok(IdempotentWrite {
                            value: record,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    insert_pipeline_head(transaction, &write.record).await?;
                    insert_pipeline_release(
                        transaction,
                        &write.record.release,
                        write.record.created_at,
                    )
                    .await?;
                    persist_knowledge_pipeline_side_effects(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                        "knowledge.pipeline.created",
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

    async fn publish_write(
        &self,
        write: PublishKnowledgePipelineWrite,
    ) -> Result<IdempotentWrite<KnowledgePipelineRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<KnowledgePipelineWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        let record = load_pipeline_head(
                            transaction,
                            reference.value.organization_id.as_uuid(),
                            reference.value.pipeline_id,
                            false,
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "KnowledgePipeline idempotency reference is missing".into(),
                            )
                        })?;
                        return Ok(IdempotentWrite {
                            value: record,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let organization_id = write.record.release.spec().organization_id.as_uuid();
                    let pipeline_id = write.record.release.spec().pipeline_id.as_uuid();
                    let current =
                        load_pipeline_head(transaction, organization_id, pipeline_id, true)
                            .await?
                            .ok_or(RepositoryError::NotFound)
                            .map_err(PostgresPersistenceError::Repository)?;
                    let updated = current
                        .publish(
                            write.record.release.clone(),
                            &write.expected_release_digest,
                            write.record.updated_at,
                        )
                        .map_err(|error| {
                            PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                        })?;
                    insert_pipeline_release(transaction, &updated.release, updated.updated_at)
                        .await?;
                    update_pipeline_head(transaction, &updated).await?;
                    persist_knowledge_pipeline_side_effects(
                        transaction,
                        &updated,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                        "knowledge.pipeline.published",
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: updated,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn insert_base_head(
    transaction: &PostgresTransaction,
    record: &KnowledgeBaseRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.revision.spec();
    let generation = i64::try_from(spec.generation).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "KnowledgeBase generation exceeds database bounds".into(),
        )
    })?;
    let rows = execute(
        transaction,
        sql_query::<()>("insert into knowledge_bases (organization_id, project_id, knowledge_base_id, current_revision_id, current_generation, current_revision_digest, created_at, updated_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.knowledge_base_id.as_uuid())
            .append(", ")
            .bind(spec.revision_id.as_uuid())
            .append(", ")
            .bind(generation)
            .append(", ")
            .bind(record.revision.digest().as_str())
            .append(", ")
            .bind(record.created_at)
            .append(", ")
            .bind(record.updated_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("KnowledgeBase", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("KnowledgeBase already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn insert_base_revision(
    transaction: &PostgresTransaction,
    revision: &KnowledgeBaseRevisionV1,
    parent_revision_id: Option<Uuid>,
    parent_digest: Option<&str>,
    created_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let spec = revision.spec();
    let generation = i64::try_from(spec.generation).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "KnowledgeBaseRevision generation exceeds database bounds".into(),
        )
    })?;
    let rows = execute(
        transaction,
        sql_query::<()>("insert into knowledge_base_revisions (organization_id, knowledge_base_id, revision_id, generation, parent_revision_id, parent_digest, revision_digest, revision_acl, created_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.knowledge_base_id.as_uuid())
            .append(", ")
            .bind(spec.revision_id.as_uuid())
            .append(", ")
            .bind(generation)
            .append(", ")
            .bind(parent_revision_id)
            .append(", ")
            .bind(parent_digest)
            .append(", ")
            .bind(revision.digest().as_str())
            .append(", ")
            .bind(revision.canonical_acl())
            .append(", ")
            .bind(created_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("KnowledgeBaseRevision", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("KnowledgeBaseRevision already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn update_base_head(
    transaction: &PostgresTransaction,
    record: &KnowledgeBaseRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.revision.spec();
    let generation = i64::try_from(spec.generation).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "KnowledgeBase generation exceeds database bounds".into(),
        )
    })?;
    let rows = execute(
        transaction,
        sql_query::<()>("update knowledge_bases set current_revision_id = ")
            .bind(spec.revision_id.as_uuid())
            .append(", current_generation = ")
            .bind(generation)
            .append(", current_revision_digest = ")
            .bind(record.revision.digest().as_str())
            .append(", updated_at = ")
            .bind(record.updated_at)
            .append(" where organization_id = ")
            .bind(spec.organization_id.as_uuid())
            .append(" and knowledge_base_id = ")
            .bind(spec.knowledge_base_id.as_uuid()),
    )
    .await?;
    require_one_row("KnowledgeBase head update", rows)
}

async fn load_base_head(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    knowledge_base_id: Uuid,
    for_update: bool,
) -> Result<Option<KnowledgeBaseRecord>, PostgresPersistenceError> {
    let mut query = sql_query::<KnowledgeBaseHeadRow>(SELECT_BASE_HEAD)
        .append(" where organization_id = ")
        .bind(organization_id)
        .append(" and knowledge_base_id = ")
        .bind(knowledge_base_id);
    if for_update {
        query = query.append(" for update");
    }
    let Some(row) = fetch_optional(transaction, query).await? else {
        return Ok(None);
    };
    let revision = load_base_revision(
        transaction,
        row.organization_id,
        row.knowledge_base_id,
        row.current_revision_id,
        false,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("KnowledgeBase head revision is missing".into())
    })?;
    if row.current_generation
        != i64::try_from(revision.spec().generation).map_err(|_| {
            PostgresPersistenceError::Invariant(
                "KnowledgeBase generation exceeds database bounds".into(),
            )
        })?
        || row.current_revision_digest != revision.digest().as_str()
        || row.project_id != revision.spec().project_id.as_uuid()
    {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgeBase head projection drifted".into(),
        ));
    }
    let record = KnowledgeBaseRecord {
        revision,
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    record
        .revision
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    Ok(Some(record))
}

async fn load_base_revision(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    knowledge_base_id: Uuid,
    revision_id: Uuid,
    for_update: bool,
) -> Result<Option<KnowledgeBaseRevisionV1>, PostgresPersistenceError> {
    let mut query = sql_query::<KnowledgeBaseRevisionRow>(SELECT_BASE_REVISION)
        .append(" where organization_id = ")
        .bind(organization_id)
        .append(" and knowledge_base_id = ")
        .bind(knowledge_base_id)
        .append(" and revision_id = ")
        .bind(revision_id);
    if for_update {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(decode_base_revision)
        .transpose()
}

fn decode_base_revision(
    row: KnowledgeBaseRevisionRow,
) -> Result<KnowledgeBaseRevisionV1, PostgresPersistenceError> {
    if row.generation <= 0 || row.parent_revision_id.is_some() != row.parent_digest.is_some() {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgeBaseRevision lineage columns are invalid".into(),
        ));
    }
    if (row.generation == 1) != row.parent_revision_id.is_none() {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgeBaseRevision parentage does not match generation".into(),
        ));
    }
    let revision = KnowledgeBaseRevisionV1::restore(&row.revision_acl, &row.revision_digest)
        .map_err(PostgresPersistenceError::Invariant)?;
    let spec = revision.spec();
    if spec.organization_id.as_uuid() != row.organization_id
        || spec.knowledge_base_id.as_uuid() != row.knowledge_base_id
        || spec.revision_id.as_uuid() != row.revision_id
        || i64::try_from(spec.generation).ok() != Some(row.generation)
    {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgeBaseRevision projection drifted".into(),
        ));
    }
    Ok(revision)
}

async fn insert_pipeline_head(
    transaction: &PostgresTransaction,
    record: &KnowledgePipelineRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.release.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into knowledge_pipelines (organization_id, project_id, pipeline_id, current_release_id, current_release_digest, created_at, updated_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.pipeline_id.as_uuid())
            .append(", ")
            .bind(spec.release_id.as_uuid())
            .append(", ")
            .bind(record.release.digest().as_str())
            .append(", ")
            .bind(record.created_at)
            .append(", ")
            .bind(record.updated_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("KnowledgePipeline", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("KnowledgePipeline already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn insert_pipeline_release(
    transaction: &PostgresTransaction,
    release: &KnowledgePipelineReleaseV1,
    created_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let spec = release.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into knowledge_pipeline_releases (organization_id, pipeline_id, release_id, release_digest, release_acl, created_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.pipeline_id.as_uuid())
            .append(", ")
            .bind(spec.release_id.as_uuid())
            .append(", ")
            .bind(release.digest().as_str())
            .append(", ")
            .bind(release.canonical_acl())
            .append(", ")
            .bind(created_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("KnowledgePipelineRelease", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("KnowledgePipelineRelease already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn update_pipeline_head(
    transaction: &PostgresTransaction,
    record: &KnowledgePipelineRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.release.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("update knowledge_pipelines set current_release_id = ")
            .bind(spec.release_id.as_uuid())
            .append(", current_release_digest = ")
            .bind(record.release.digest().as_str())
            .append(", updated_at = ")
            .bind(record.updated_at)
            .append(" where organization_id = ")
            .bind(spec.organization_id.as_uuid())
            .append(" and pipeline_id = ")
            .bind(spec.pipeline_id.as_uuid()),
    )
    .await?;
    require_one_row("KnowledgePipeline head update", rows)
}

async fn load_pipeline_head(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    pipeline_id: Uuid,
    for_update: bool,
) -> Result<Option<KnowledgePipelineRecord>, PostgresPersistenceError> {
    let mut query = sql_query::<KnowledgePipelineHeadRow>(SELECT_PIPELINE_HEAD)
        .append(" where organization_id = ")
        .bind(organization_id)
        .append(" and pipeline_id = ")
        .bind(pipeline_id);
    if for_update {
        query = query.append(" for update");
    }
    let Some(row) = fetch_optional(transaction, query).await? else {
        return Ok(None);
    };
    let release = load_pipeline_release(
        transaction,
        row.organization_id,
        row.pipeline_id,
        row.current_release_id,
        false,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("KnowledgePipeline head release is missing".into())
    })?;
    if row.current_release_digest != release.digest().as_str()
        || row.project_id != release.spec().project_id.as_uuid()
    {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgePipeline head projection drifted".into(),
        ));
    }
    let record = KnowledgePipelineRecord {
        release,
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    record
        .release
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    Ok(Some(record))
}

async fn load_pipeline_release(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    pipeline_id: Uuid,
    release_id: Uuid,
    for_update: bool,
) -> Result<Option<KnowledgePipelineReleaseV1>, PostgresPersistenceError> {
    let mut query = sql_query::<KnowledgePipelineReleaseRow>(SELECT_PIPELINE_RELEASE)
        .append(" where organization_id = ")
        .bind(organization_id)
        .append(" and pipeline_id = ")
        .bind(pipeline_id)
        .append(" and release_id = ")
        .bind(release_id);
    if for_update {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(decode_pipeline_release)
        .transpose()
}

fn decode_pipeline_release(
    row: KnowledgePipelineReleaseRow,
) -> Result<KnowledgePipelineReleaseV1, PostgresPersistenceError> {
    let release = KnowledgePipelineReleaseV1::restore(&row.release_acl, &row.release_digest)
        .map_err(PostgresPersistenceError::Invariant)?;
    let spec = release.spec();
    if spec.organization_id.as_uuid() != row.organization_id
        || spec.pipeline_id.as_uuid() != row.pipeline_id
        || spec.release_id.as_uuid() != row.release_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgePipelineRelease projection drifted".into(),
        ));
    }
    Ok(release)
}

struct KnowledgeBaseHeadRow {
    organization_id: Uuid,
    project_id: Uuid,
    knowledge_base_id: Uuid,
    current_revision_id: Uuid,
    current_generation: i64,
    current_revision_digest: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for KnowledgeBaseHeadRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            knowledge_base_id: decode(row, 2)?,
            current_revision_id: decode(row, 3)?,
            current_generation: decode(row, 4)?,
            current_revision_digest: decode(row, 5)?,
            created_at: decode(row, 6)?,
            updated_at: decode(row, 7)?,
        })
    }
}

struct KnowledgeBaseRevisionRow {
    organization_id: Uuid,
    knowledge_base_id: Uuid,
    revision_id: Uuid,
    generation: i64,
    parent_revision_id: Option<Uuid>,
    parent_digest: Option<String>,
    revision_digest: String,
    revision_acl: String,
}

impl FromRow for KnowledgeBaseRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            knowledge_base_id: decode(row, 1)?,
            revision_id: decode(row, 2)?,
            generation: decode(row, 3)?,
            parent_revision_id: decode(row, 4)?,
            parent_digest: decode(row, 5)?,
            revision_digest: decode(row, 6)?,
            revision_acl: decode(row, 7)?,
        })
    }
}

struct KnowledgePipelineHeadRow {
    organization_id: Uuid,
    project_id: Uuid,
    pipeline_id: Uuid,
    current_release_id: Uuid,
    current_release_digest: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for KnowledgePipelineHeadRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            pipeline_id: decode(row, 2)?,
            current_release_id: decode(row, 3)?,
            current_release_digest: decode(row, 4)?,
            created_at: decode(row, 5)?,
            updated_at: decode(row, 6)?,
        })
    }
}

struct KnowledgePipelineReleaseRow {
    organization_id: Uuid,
    pipeline_id: Uuid,
    release_id: Uuid,
    release_digest: String,
    release_acl: String,
}

impl FromRow for KnowledgePipelineReleaseRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            pipeline_id: decode(row, 1)?,
            release_id: decode(row, 2)?,
            release_digest: decode(row, 3)?,
            release_acl: decode(row, 4)?,
        })
    }
}


async fn persist_knowledge_base_side_effects(
    transaction: &PostgresTransaction,
    record: &KnowledgeBaseRecord,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    idempotency: &IdempotencyRequest,
    action: &'static str,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.revision.spec();
    store_outbox(transaction, event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: spec.knowledge_base_id.as_uuid(),
            occurred_at: record.updated_at,
            request_id,
            scope: AuditWrite::resource_scope(
                spec.organization_id.as_uuid(),
                spec.project_id,
                None,
            ),
            details: serde_json::json!({
                "projectId": spec.project_id,
                "knowledgeBaseId": spec.knowledge_base_id,
                "revisionId": spec.revision_id,
                "generation": spec.generation,
                "revisionDigest": record.revision.digest().as_str(),
            }),
        },
    )
    .await?;
    store_idempotency(
        transaction,
        idempotency,
        &KnowledgeBaseWriteReference::from(record),
    )
    .await
}

async fn persist_knowledge_pipeline_side_effects(
    transaction: &PostgresTransaction,
    record: &KnowledgePipelineRecord,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    idempotency: &IdempotencyRequest,
    action: &'static str,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.release.spec();
    store_outbox(transaction, event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: spec.pipeline_id.as_uuid(),
            occurred_at: record.updated_at,
            request_id,
            scope: AuditWrite::resource_scope(
                spec.organization_id.as_uuid(),
                spec.project_id,
                None,
            ),
            details: serde_json::json!({
                "projectId": spec.project_id,
                "pipelineId": spec.pipeline_id,
                "releaseId": spec.release_id,
                "releaseDigest": record.release.digest().as_str(),
            }),
        },
    )
    .await?;
    store_idempotency(
        transaction,
        idempotency,
        &KnowledgePipelineWriteReference::from(record),
    )
    .await
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn repository_restores_acl_through_contract_parser() {
        let source = include_str!("knowledge_postgres.rs");
        assert!(source.contains("KnowledgeBaseRevisionV1::restore"));
        assert!(source.contains("KnowledgePipelineReleaseV1::restore"));
        assert!(source.contains("for update"));
    }
}
