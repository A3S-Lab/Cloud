use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::knowledge::domain::{
    CreateKnowledgeChunk, CreateKnowledgeChunkWrite, CreateKnowledgeDocument,
    CreateKnowledgeDocumentWrite, IKnowledgeChunkRepository, IKnowledgeDocumentRepository,
    KnowledgeChunkRecord, KnowledgeChunkV1, KnowledgeChunkWriteReference, KnowledgeDocumentRecord,
    KnowledgeDocumentV1, KnowledgeDocumentWriteReference,
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

const SELECT_DOCUMENT: &str = "select organization_id, project_id, knowledge_base_id, knowledge_base_revision_id, document_id, document_digest, document_acl, created_at from knowledge_documents";
const SELECT_CHUNK: &str = "select organization_id, project_id, document_id, chunk_id, chunk_digest, chunk_acl, created_at from knowledge_chunks";

/// Durable KnowledgeDocument catalog.
#[derive(Clone)]
pub struct PostgresKnowledgeDocumentRepository {
    executor: PostgresExecutor,
}

impl PostgresKnowledgeDocumentRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IKnowledgeDocumentRepository for PostgresKnowledgeDocumentRepository {
    async fn create(
        &self,
        request: CreateKnowledgeDocument,
    ) -> Result<KnowledgeDocumentRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let record = KnowledgeDocumentRecord::new(request.document, request.created_at)
                        .map_err(RepositoryError::Conflict)?;
                    insert_document(transaction, &record).await?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        document_id: Uuid,
    ) -> Result<Option<KnowledgeDocumentRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(
                    async move { load_document(transaction, organization_id, document_id).await },
                )
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_for_knowledge_base(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeDocumentRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict("KnowledgeDocument list limit is out of bounds".into())
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<KnowledgeDocumentRow, _>(
                        transaction,
                        sql_query::<KnowledgeDocumentRow>(SELECT_DOCUMENT)
                            .append(" where organization_id = ")
                            .bind(organization_id)
                            .append(" and knowledge_base_id = ")
                            .bind(knowledge_base_id)
                            .append(" order by document_id asc limit ")
                            .bind(limit),
                    )
                    .await?;
                    rows.into_iter().map(decode_document).collect()
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<KnowledgeDocumentRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) = idempotency_replay::<KnowledgeDocumentWriteReference>(
                        transaction,
                        &idempotency,
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    load_document(
                        transaction,
                        reference.value.organization_id.as_uuid(),
                        reference.value.document_id,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_write(
        &self,
        write: CreateKnowledgeDocumentWrite,
    ) -> Result<IdempotentWrite<KnowledgeDocumentRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<KnowledgeDocumentWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        let record = load_document(
                            transaction,
                            reference.value.organization_id.as_uuid(),
                            reference.value.document_id,
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "KnowledgeDocument idempotency reference is missing".into(),
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
                    insert_document(transaction, &write.record).await?;
                    persist_knowledge_document_side_effects(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                        "knowledge.document.created",
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
}

/// Durable KnowledgeChunk catalog.
#[derive(Clone)]
pub struct PostgresKnowledgeChunkRepository {
    executor: PostgresExecutor,
}

impl PostgresKnowledgeChunkRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IKnowledgeChunkRepository for PostgresKnowledgeChunkRepository {
    async fn create(
        &self,
        request: CreateKnowledgeChunk,
    ) -> Result<KnowledgeChunkRecord, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let record = KnowledgeChunkRecord::new(request.chunk, request.created_at)
                        .map_err(RepositoryError::Conflict)?;
                    insert_chunk(transaction, &record).await?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        chunk_id: Uuid,
    ) -> Result<Option<KnowledgeChunkRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { load_chunk(transaction, organization_id, chunk_id).await })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_for_document(
        &self,
        organization_id: Uuid,
        document_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeChunkRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict("KnowledgeChunk list limit is out of bounds".into())
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<KnowledgeChunkRow, _>(
                        transaction,
                        sql_query::<KnowledgeChunkRow>(SELECT_CHUNK)
                            .append(" where organization_id = ")
                            .bind(organization_id)
                            .append(" and document_id = ")
                            .bind(document_id)
                            .append(" order by chunk_id asc limit ")
                            .bind(limit),
                    )
                    .await?;
                    rows.into_iter().map(decode_chunk).collect()
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<KnowledgeChunkRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) = idempotency_replay::<KnowledgeChunkWriteReference>(
                        transaction,
                        &idempotency,
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    load_chunk(
                        transaction,
                        reference.value.organization_id.as_uuid(),
                        reference.value.chunk_id,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create_write(
        &self,
        write: CreateKnowledgeChunkWrite,
    ) -> Result<IdempotentWrite<KnowledgeChunkRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<KnowledgeChunkWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        let record = load_chunk(
                            transaction,
                            reference.value.organization_id.as_uuid(),
                            reference.value.chunk_id,
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "KnowledgeChunk idempotency reference is missing".into(),
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
                    insert_chunk(transaction, &write.record).await?;
                    persist_knowledge_chunk_side_effects(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                        "knowledge.chunk.created",
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
}

async fn insert_document(
    transaction: &PostgresTransaction,
    record: &KnowledgeDocumentRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.document.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into knowledge_documents (organization_id, project_id, knowledge_base_id, knowledge_base_revision_id, document_id, document_digest, document_acl, created_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.knowledge_base_id.as_uuid())
            .append(", ")
            .bind(spec.knowledge_base_revision_id.as_uuid())
            .append(", ")
            .bind(spec.document_id.as_uuid())
            .append(", ")
            .bind(record.document.digest().as_str())
            .append(", ")
            .bind(record.document.canonical_acl())
            .append(", ")
            .bind(record.created_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("KnowledgeDocument", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("KnowledgeDocument already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn insert_chunk(
    transaction: &PostgresTransaction,
    record: &KnowledgeChunkRecord,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.chunk.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into knowledge_chunks (organization_id, project_id, document_id, chunk_id, chunk_digest, chunk_acl, created_at) values (")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.document_id.as_uuid())
            .append(", ")
            .bind(spec.chunk_id.as_uuid())
            .append(", ")
            .bind(record.chunk.digest().as_str())
            .append(", ")
            .bind(record.chunk.canonical_acl())
            .append(", ")
            .bind(record.created_at)
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("KnowledgeChunk", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("KnowledgeChunk already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn load_document(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<Option<KnowledgeDocumentRecord>, PostgresPersistenceError> {
    let row = fetch_optional::<KnowledgeDocumentRow, _>(
        transaction,
        sql_query::<KnowledgeDocumentRow>(SELECT_DOCUMENT)
            .append(" where organization_id = ")
            .bind(organization_id)
            .append(" and document_id = ")
            .bind(document_id),
    )
    .await?;
    row.map(decode_document).transpose()
}

async fn load_chunk(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    chunk_id: Uuid,
) -> Result<Option<KnowledgeChunkRecord>, PostgresPersistenceError> {
    let row = fetch_optional::<KnowledgeChunkRow, _>(
        transaction,
        sql_query::<KnowledgeChunkRow>(SELECT_CHUNK)
            .append(" where organization_id = ")
            .bind(organization_id)
            .append(" and chunk_id = ")
            .bind(chunk_id),
    )
    .await?;
    row.map(decode_chunk).transpose()
}

async fn persist_knowledge_document_side_effects(
    transaction: &PostgresTransaction,
    record: &KnowledgeDocumentRecord,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    idempotency: &IdempotencyRequest,
    action: &'static str,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.document.spec();
    store_outbox(transaction, event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: spec.document_id.as_uuid(),
            occurred_at: record.created_at,
            request_id,
            scope: AuditWrite::resource_scope(
                spec.organization_id.as_uuid(),
                spec.project_id,
                None,
            ),
            details: serde_json::json!({
                "projectId": spec.project_id,
                "knowledgeBaseId": spec.knowledge_base_id,
                "documentId": spec.document_id,
                "documentDigest": record.document.digest().as_str(),
            }),
        },
    )
    .await?;
    store_idempotency(
        transaction,
        idempotency,
        &KnowledgeDocumentWriteReference::from(record),
    )
    .await
}

async fn persist_knowledge_chunk_side_effects(
    transaction: &PostgresTransaction,
    record: &KnowledgeChunkRecord,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    idempotency: &IdempotencyRequest,
    action: &'static str,
) -> Result<(), PostgresPersistenceError> {
    let spec = record.chunk.spec();
    store_outbox(transaction, event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: spec.chunk_id.as_uuid(),
            occurred_at: record.created_at,
            request_id,
            scope: AuditWrite::resource_scope(
                spec.organization_id.as_uuid(),
                spec.project_id,
                None,
            ),
            details: serde_json::json!({
                "projectId": spec.project_id,
                "documentId": spec.document_id,
                "chunkId": spec.chunk_id,
                "chunkDigest": record.chunk.digest().as_str(),
            }),
        },
    )
    .await?;
    store_idempotency(
        transaction,
        idempotency,
        &KnowledgeChunkWriteReference::from(record),
    )
    .await
}

fn decode_document(
    row: KnowledgeDocumentRow,
) -> Result<KnowledgeDocumentRecord, PostgresPersistenceError> {
    let document = KnowledgeDocumentV1::restore(&row.document_acl, &row.document_digest)
        .map_err(PostgresPersistenceError::Invariant)?;
    let spec = document.spec();
    if spec.organization_id.as_uuid() != row.organization_id
        || spec.project_id.as_uuid() != row.project_id
        || spec.knowledge_base_id.as_uuid() != row.knowledge_base_id
        || spec.knowledge_base_revision_id.as_uuid() != row.knowledge_base_revision_id
        || spec.document_id.as_uuid() != row.document_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgeDocument projection drifted".into(),
        ));
    }
    KnowledgeDocumentRecord::new(document, row.created_at)
        .map_err(PostgresPersistenceError::Invariant)
}

fn decode_chunk(row: KnowledgeChunkRow) -> Result<KnowledgeChunkRecord, PostgresPersistenceError> {
    let chunk = KnowledgeChunkV1::restore(&row.chunk_acl, &row.chunk_digest)
        .map_err(PostgresPersistenceError::Invariant)?;
    let spec = chunk.spec();
    if spec.organization_id.as_uuid() != row.organization_id
        || spec.project_id.as_uuid() != row.project_id
        || spec.document_id.as_uuid() != row.document_id
        || spec.chunk_id.as_uuid() != row.chunk_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "KnowledgeChunk projection drifted".into(),
        ));
    }
    KnowledgeChunkRecord::new(chunk, row.created_at).map_err(PostgresPersistenceError::Invariant)
}

struct KnowledgeDocumentRow {
    organization_id: Uuid,
    project_id: Uuid,
    knowledge_base_id: Uuid,
    knowledge_base_revision_id: Uuid,
    document_id: Uuid,
    document_digest: String,
    document_acl: String,
    created_at: DateTime<Utc>,
}

impl FromRow for KnowledgeDocumentRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            knowledge_base_id: decode(row, 2)?,
            knowledge_base_revision_id: decode(row, 3)?,
            document_id: decode(row, 4)?,
            document_digest: decode(row, 5)?,
            document_acl: decode(row, 6)?,
            created_at: decode(row, 7)?,
        })
    }
}

struct KnowledgeChunkRow {
    organization_id: Uuid,
    project_id: Uuid,
    document_id: Uuid,
    chunk_id: Uuid,
    chunk_digest: String,
    chunk_acl: String,
    created_at: DateTime<Utc>,
}

impl FromRow for KnowledgeChunkRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            document_id: decode(row, 2)?,
            chunk_id: decode(row, 3)?,
            chunk_digest: decode(row, 4)?,
            chunk_acl: decode(row, 5)?,
            created_at: decode(row, 6)?,
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

#[cfg(test)]
mod tests {
    #[test]
    fn document_repository_restores_acl_through_contract_parser() {
        let source = include_str!("knowledge_document_postgres.rs");
        assert!(source.contains("KnowledgeDocumentV1::restore"));
        assert!(source.contains("KnowledgeChunkV1::restore"));
    }
}
