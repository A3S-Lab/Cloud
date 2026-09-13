use super::{KnowledgeBaseRevisionV1, KnowledgePipelineReleaseV1};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBaseRecord {
    pub revision: KnowledgeBaseRevisionV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl KnowledgeBaseRecord {
    pub fn new(
        revision: KnowledgeBaseRevisionV1,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_timestamp(created_at)?;
        if revision.spec().generation != 1 {
            return Err("KnowledgeBase create requires generation 1".into());
        }
        revision.validate()?;
        Ok(Self {
            revision,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn append(
        &self,
        revision: KnowledgeBaseRevisionV1,
        expected_revision_digest: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_timestamp(updated_at)?;
        if updated_at < self.updated_at {
            return Err("KnowledgeBase update time cannot move backwards".into());
        }
        if self.revision.digest().as_str() != expected_revision_digest {
            return Err("KnowledgeBase head is stale".into());
        }
        let current = self.revision.spec();
        let next = revision.spec();
        if next.organization_id != current.organization_id
            || next.project_id != current.project_id
            || next.knowledge_base_id != current.knowledge_base_id
        {
            return Err("KnowledgeBase identity cannot change across revisions".into());
        }
        if next.generation != current.generation + 1 {
            return Err("KnowledgeBase generation must advance by exactly one".into());
        }
        if next.revision_id == current.revision_id {
            return Err("KnowledgeBase revision identity must change".into());
        }
        revision.validate()?;
        Ok(Self {
            revision,
            created_at: self.created_at,
            updated_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateKnowledgeBase {
    pub revision: KnowledgeBaseRevisionV1,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendKnowledgeBaseRevision {
    pub organization_id: Uuid,
    pub knowledge_base_id: Uuid,
    pub expected_revision_digest: String,
    pub revision: KnowledgeBaseRevisionV1,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait IKnowledgeBaseRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateKnowledgeBase,
    ) -> Result<KnowledgeBaseRecord, RepositoryError>;

    async fn find(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
    ) -> Result<Option<KnowledgeBaseRecord>, RepositoryError>;

    async fn list(&self, limit: usize) -> Result<Vec<KnowledgeBaseRecord>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        revision_id: Uuid,
    ) -> Result<Option<KnowledgeBaseRevisionV1>, RepositoryError>;

    async fn append_revision(
        &self,
        request: AppendKnowledgeBaseRevision,
    ) -> Result<KnowledgeBaseRecord, RepositoryError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePipelineRecord {
    pub release: KnowledgePipelineReleaseV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl KnowledgePipelineRecord {
    pub fn new(
        release: KnowledgePipelineReleaseV1,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_timestamp(created_at)?;
        release.validate()?;
        Ok(Self {
            release,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn publish(
        &self,
        release: KnowledgePipelineReleaseV1,
        expected_release_digest: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_timestamp(updated_at)?;
        if updated_at < self.updated_at {
            return Err("KnowledgePipeline update time cannot move backwards".into());
        }
        if self.release.digest().as_str() != expected_release_digest {
            return Err("KnowledgePipeline head is stale".into());
        }
        let current = self.release.spec();
        let next = release.spec();
        if next.organization_id != current.organization_id
            || next.project_id != current.project_id
            || next.pipeline_id != current.pipeline_id
        {
            return Err("KnowledgePipeline identity cannot change across releases".into());
        }
        if next.release_id == current.release_id {
            return Err("KnowledgePipeline release identity must change".into());
        }
        release.validate()?;
        Ok(Self {
            release,
            created_at: self.created_at,
            updated_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateKnowledgePipeline {
    pub release: KnowledgePipelineReleaseV1,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishKnowledgePipelineRelease {
    pub organization_id: Uuid,
    pub pipeline_id: Uuid,
    pub expected_release_digest: String,
    pub release: KnowledgePipelineReleaseV1,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait IKnowledgePipelineRepository: Send + Sync {
    async fn create(
        &self,
        request: CreateKnowledgePipeline,
    ) -> Result<KnowledgePipelineRecord, RepositoryError>;

    async fn find(
        &self,
        organization_id: Uuid,
        pipeline_id: Uuid,
    ) -> Result<Option<KnowledgePipelineRecord>, RepositoryError>;

    async fn find_release(
        &self,
        organization_id: Uuid,
        pipeline_id: Uuid,
        release_id: Uuid,
    ) -> Result<Option<KnowledgePipelineReleaseV1>, RepositoryError>;

    async fn publish_release(
        &self,
        request: PublishKnowledgePipelineRelease,
    ) -> Result<KnowledgePipelineRecord, RepositoryError>;
}

fn validate_timestamp(value: DateTime<Utc>) -> Result<(), String> {
    if value.timestamp_subsec_nanos() != 0 {
        return Err("Knowledge catalog timestamps must use whole seconds".into());
    }
    Ok(())
}
