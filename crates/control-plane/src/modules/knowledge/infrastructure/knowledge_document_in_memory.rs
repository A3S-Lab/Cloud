use crate::modules::knowledge::domain::{
    CreateKnowledgeChunk, CreateKnowledgeChunkWrite, CreateKnowledgeDocument,
    CreateKnowledgeDocumentWrite, IKnowledgeChunkRepository, IKnowledgeDocumentRepository,
    KnowledgeChunkRecord, KnowledgeChunkWriteReference, KnowledgeDocumentRecord,
    KnowledgeDocumentWriteReference,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, IdempotentWrite, RepositoryError};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Deterministic local adapter for immutable KnowledgeDocument records.
pub struct InMemoryKnowledgeDocumentRepository {
    documents: RwLock<BTreeMap<(Uuid, Uuid), KnowledgeDocumentRecord>>,
    idempotency: RwLock<BTreeMap<(String, String), KnowledgeDocumentWriteReference>>,
}

impl Default for InMemoryKnowledgeDocumentRepository {
    fn default() -> Self {
        Self {
            documents: RwLock::new(BTreeMap::new()),
            idempotency: RwLock::new(BTreeMap::new()),
        }
    }
}

impl InMemoryKnowledgeDocumentRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IKnowledgeDocumentRepository for InMemoryKnowledgeDocumentRepository {
    async fn create(
        &self,
        request: CreateKnowledgeDocument,
    ) -> Result<KnowledgeDocumentRecord, RepositoryError> {
        let record = KnowledgeDocumentRecord::new(request.document, request.created_at)
            .map_err(RepositoryError::Conflict)?;
        let key = (
            record.document.spec().organization_id.as_uuid(),
            record.document.spec().document_id.as_uuid(),
        );
        let mut documents = self.documents.write().await;
        if documents.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "KnowledgeDocument already exists".into(),
            ));
        }
        documents.insert(key, record.clone());
        Ok(record)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        document_id: Uuid,
    ) -> Result<Option<KnowledgeDocumentRecord>, RepositoryError> {
        Ok(self
            .documents
            .read()
            .await
            .get(&(organization_id, document_id))
            .cloned())
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
        Ok(self
            .documents
            .read()
            .await
            .values()
            .filter(|record| {
                record.document.spec().organization_id.as_uuid() == organization_id
                    && record.document.spec().knowledge_base_id.as_uuid() == knowledge_base_id
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<KnowledgeDocumentRecord>, RepositoryError> {
        let key = (idempotency.scope.clone(), idempotency.key.clone());
        let Some(reference) = self.idempotency.read().await.get(&key).cloned() else {
            return Ok(None);
        };
        self.find(reference.organization_id.as_uuid(), reference.document_id)
            .await
    }

    async fn create_write(
        &self,
        write: CreateKnowledgeDocumentWrite,
    ) -> Result<IdempotentWrite<KnowledgeDocumentRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        if let Some(existing) = self.replay_write(&write.idempotency).await? {
            return Ok(IdempotentWrite {
                value: existing,
                replayed: true,
            });
        }
        let record = self
            .create(CreateKnowledgeDocument {
                document: write.record.document.clone(),
                created_at: write.record.created_at,
            })
            .await?;
        let reference = KnowledgeDocumentWriteReference::from(&record);
        self.idempotency.write().await.insert(
            (
                write.idempotency.scope.clone(),
                write.idempotency.key.clone(),
            ),
            reference,
        );
        Ok(IdempotentWrite {
            value: record,
            replayed: false,
        })
    }
}

/// Deterministic local adapter for immutable KnowledgeChunk records.
pub struct InMemoryKnowledgeChunkRepository {
    chunks: RwLock<BTreeMap<(Uuid, Uuid), KnowledgeChunkRecord>>,
    idempotency: RwLock<BTreeMap<(String, String), KnowledgeChunkWriteReference>>,
}

impl Default for InMemoryKnowledgeChunkRepository {
    fn default() -> Self {
        Self {
            chunks: RwLock::new(BTreeMap::new()),
            idempotency: RwLock::new(BTreeMap::new()),
        }
    }
}

impl InMemoryKnowledgeChunkRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IKnowledgeChunkRepository for InMemoryKnowledgeChunkRepository {
    async fn create(
        &self,
        request: CreateKnowledgeChunk,
    ) -> Result<KnowledgeChunkRecord, RepositoryError> {
        let record = KnowledgeChunkRecord::new(request.chunk, request.created_at)
            .map_err(RepositoryError::Conflict)?;
        let key = (
            record.chunk.spec().organization_id.as_uuid(),
            record.chunk.spec().chunk_id.as_uuid(),
        );
        let mut chunks = self.chunks.write().await;
        if chunks.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "KnowledgeChunk already exists".into(),
            ));
        }
        chunks.insert(key, record.clone());
        Ok(record)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        chunk_id: Uuid,
    ) -> Result<Option<KnowledgeChunkRecord>, RepositoryError> {
        Ok(self
            .chunks
            .read()
            .await
            .get(&(organization_id, chunk_id))
            .cloned())
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
        Ok(self
            .chunks
            .read()
            .await
            .values()
            .filter(|record| {
                record.chunk.spec().organization_id.as_uuid() == organization_id
                    && record.chunk.spec().document_id.as_uuid() == document_id
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<KnowledgeChunkRecord>, RepositoryError> {
        let key = (idempotency.scope.clone(), idempotency.key.clone());
        let Some(reference) = self.idempotency.read().await.get(&key).cloned() else {
            return Ok(None);
        };
        self.find(reference.organization_id.as_uuid(), reference.chunk_id)
            .await
    }

    async fn create_write(
        &self,
        write: CreateKnowledgeChunkWrite,
    ) -> Result<IdempotentWrite<KnowledgeChunkRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        if let Some(existing) = self.replay_write(&write.idempotency).await? {
            return Ok(IdempotentWrite {
                value: existing,
                replayed: true,
            });
        }
        let record = self
            .create(CreateKnowledgeChunk {
                chunk: write.record.chunk.clone(),
                created_at: write.record.created_at,
            })
            .await?;
        let reference = KnowledgeChunkWriteReference::from(&record);
        self.idempotency.write().await.insert(
            (
                write.idempotency.scope.clone(),
                write.idempotency.key.clone(),
            ),
            reference,
        );
        Ok(IdempotentWrite {
            value: record,
            replayed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::knowledge::domain::{KnowledgeChunkV1, KnowledgeDocumentV1};
    use chrono::{DateTime, Utc};

    const DOCUMENT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-document.acl"
    ));
    const CHUNK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-chunk.acl"
    ));

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    #[tokio::test]
    async fn knowledge_document_catalog_creates_and_rejects_duplicates() {
        let repository = InMemoryKnowledgeDocumentRepository::new();
        let document = KnowledgeDocumentV1::parse_acl(DOCUMENT).expect("document");
        let created = repository
            .create(CreateKnowledgeDocument {
                document: document.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(created.document, document);
        assert_eq!(
            repository
                .list_for_knowledge_base(
                    document.spec().organization_id.as_uuid(),
                    document.spec().knowledge_base_id.as_uuid(),
                    1,
                )
                .await
                .expect("list")
                .len(),
            1
        );
        let conflict = repository
            .create(CreateKnowledgeDocument {
                document,
                created_at: timestamp(1_001),
            })
            .await;
        assert!(matches!(conflict, Err(RepositoryError::Conflict(_))));
    }

    #[tokio::test]
    async fn knowledge_chunk_catalog_lists_by_document() {
        let repository = InMemoryKnowledgeChunkRepository::new();
        let chunk = KnowledgeChunkV1::parse_acl(CHUNK).expect("chunk");
        repository
            .create(CreateKnowledgeChunk {
                chunk: chunk.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        let listed = repository
            .list_for_document(
                chunk.spec().organization_id.as_uuid(),
                chunk.spec().document_id.as_uuid(),
                8,
            )
            .await
            .expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].chunk, chunk);
        assert!(repository
            .list_for_document(
                chunk.spec().organization_id.as_uuid(),
                chunk.spec().document_id.as_uuid(),
                0
            )
            .await
            .expect("empty")
            .is_empty());
    }
}
