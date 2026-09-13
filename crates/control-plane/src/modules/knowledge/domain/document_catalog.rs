use super::{KnowledgeChunkV1, KnowledgeDocumentV1};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeDocumentRecord {
    pub document: KnowledgeDocumentV1,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeDocumentRecord {
    pub fn new(document: KnowledgeDocumentV1, created_at: DateTime<Utc>) -> Result<Self, String> {
        validate_timestamp(created_at)?;
        Ok(Self {
            document,
            created_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateKnowledgeDocument {
    pub document: KnowledgeDocumentV1,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait IKnowledgeDocumentRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateKnowledgeDocument,
    ) -> Result<KnowledgeDocumentRecord, RepositoryError>;

    async fn find(
        &self,
        organization_id: Uuid,
        document_id: Uuid,
    ) -> Result<Option<KnowledgeDocumentRecord>, RepositoryError>;

    async fn list_for_knowledge_base(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeDocumentRecord>, RepositoryError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChunkRecord {
    pub chunk: KnowledgeChunkV1,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeChunkRecord {
    pub fn new(chunk: KnowledgeChunkV1, created_at: DateTime<Utc>) -> Result<Self, String> {
        validate_timestamp(created_at)?;
        Ok(Self { chunk, created_at })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateKnowledgeChunk {
    pub chunk: KnowledgeChunkV1,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait IKnowledgeChunkRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateKnowledgeChunk,
    ) -> Result<KnowledgeChunkRecord, RepositoryError>;

    async fn find(
        &self,
        organization_id: Uuid,
        chunk_id: Uuid,
    ) -> Result<Option<KnowledgeChunkRecord>, RepositoryError>;

    async fn list_for_document(
        &self,
        organization_id: Uuid,
        document_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeChunkRecord>, RepositoryError>;
}

fn validate_timestamp(value: DateTime<Utc>) -> Result<(), String> {
    if value.timestamp_subsec_nanos() != 0 {
        return Err("Knowledge document timestamps must use whole seconds".into());
    }
    Ok(())
}
