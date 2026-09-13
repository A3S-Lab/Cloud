use super::writes::{
    CreateExternalKnowledgeBindingWrite, CreateKnowledgeIndexRevisionWrite,
    CreateKnowledgeRetrievalPolicyRevisionWrite,
};
use super::{
    ExternalKnowledgeBindingV1, KnowledgeIndexRevisionV1, KnowledgeRetrievalPolicyRevisionV1,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, IdempotentWrite, RepositoryError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeIndexRevisionRecord {
    pub index_revision: KnowledgeIndexRevisionV1,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeIndexRevisionRecord {
    pub fn new(
        index_revision: KnowledgeIndexRevisionV1,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_timestamp(created_at)?;
        Ok(Self {
            index_revision,
            created_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateKnowledgeIndexRevision {
    pub index_revision: KnowledgeIndexRevisionV1,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait IKnowledgeIndexRevisionRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateKnowledgeIndexRevision,
    ) -> Result<KnowledgeIndexRevisionRecord, RepositoryError>;

    async fn find(
        &self,
        organization_id: Uuid,
        index_revision_id: Uuid,
    ) -> Result<Option<KnowledgeIndexRevisionRecord>, RepositoryError>;

    async fn list_for_knowledge_base_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_revision_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeIndexRevisionRecord>, RepositoryError>;

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<KnowledgeIndexRevisionRecord>, RepositoryError>;

    async fn create_write(
        &self,
        write: CreateKnowledgeIndexRevisionWrite,
    ) -> Result<IdempotentWrite<KnowledgeIndexRevisionRecord>, RepositoryError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRetrievalPolicyRevisionRecord {
    pub policy_revision: KnowledgeRetrievalPolicyRevisionV1,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeRetrievalPolicyRevisionRecord {
    pub fn new(
        policy_revision: KnowledgeRetrievalPolicyRevisionV1,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_timestamp(created_at)?;
        Ok(Self {
            policy_revision,
            created_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateKnowledgeRetrievalPolicyRevision {
    pub policy_revision: KnowledgeRetrievalPolicyRevisionV1,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait IKnowledgeRetrievalPolicyRevisionRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateKnowledgeRetrievalPolicyRevision,
    ) -> Result<KnowledgeRetrievalPolicyRevisionRecord, RepositoryError>;

    async fn find(
        &self,
        organization_id: Uuid,
        policy_revision_id: Uuid,
    ) -> Result<Option<KnowledgeRetrievalPolicyRevisionRecord>, RepositoryError>;

    async fn list_for_knowledge_base_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_revision_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeRetrievalPolicyRevisionRecord>, RepositoryError>;

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<KnowledgeRetrievalPolicyRevisionRecord>, RepositoryError>;

    async fn create_write(
        &self,
        write: CreateKnowledgeRetrievalPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<KnowledgeRetrievalPolicyRevisionRecord>, RepositoryError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalKnowledgeBindingRecord {
    pub binding: ExternalKnowledgeBindingV1,
    pub created_at: DateTime<Utc>,
}

impl ExternalKnowledgeBindingRecord {
    pub fn new(
        binding: ExternalKnowledgeBindingV1,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_timestamp(created_at)?;
        Ok(Self { binding, created_at })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateExternalKnowledgeBinding {
    pub binding: ExternalKnowledgeBindingV1,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait IExternalKnowledgeBindingRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateExternalKnowledgeBinding,
    ) -> Result<ExternalKnowledgeBindingRecord, RepositoryError>;

    async fn find(
        &self,
        organization_id: Uuid,
        binding_id: Uuid,
    ) -> Result<Option<ExternalKnowledgeBindingRecord>, RepositoryError>;

    async fn list_for_knowledge_base(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ExternalKnowledgeBindingRecord>, RepositoryError>;

    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ExternalKnowledgeBindingRecord>, RepositoryError>;

    async fn create_write(
        &self,
        write: CreateExternalKnowledgeBindingWrite,
    ) -> Result<IdempotentWrite<ExternalKnowledgeBindingRecord>, RepositoryError>;
}

fn validate_timestamp(value: DateTime<Utc>) -> Result<(), String> {
    if value.timestamp_subsec_nanos() != 0 {
        return Err("Knowledge catalog timestamps must use whole seconds".into());
    }
    Ok(())
}
