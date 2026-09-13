use crate::modules::knowledge::domain::{
    AppendKnowledgeBaseRevision, CreateKnowledgeBase, CreateKnowledgePipeline,
    IKnowledgeBaseRepository, IKnowledgePipelineRepository, KnowledgeBaseRecord,
    KnowledgeBaseRevisionV1, KnowledgePipelineRecord, KnowledgePipelineReleaseV1,
    PublishKnowledgePipelineRelease,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Deterministic local adapter for the KnowledgeBase revision catalog.
#[derive(Default)]
pub struct InMemoryKnowledgeBaseRepository {
    heads: RwLock<BTreeMap<(Uuid, Uuid), KnowledgeBaseRecord>>,
    revisions: RwLock<BTreeMap<Uuid, KnowledgeBaseRevisionV1>>,
}

impl InMemoryKnowledgeBaseRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IKnowledgeBaseRepository for InMemoryKnowledgeBaseRepository {
    async fn create(
        &self,
        request: CreateKnowledgeBase,
    ) -> Result<KnowledgeBaseRecord, RepositoryError> {
        let record = KnowledgeBaseRecord::new(request.revision, request.created_at)
            .map_err(RepositoryError::Conflict)?;
        let key = (
            record.revision.spec().organization_id.as_uuid(),
            record.revision.spec().knowledge_base_id.as_uuid(),
        );
        let revision_id = record.revision.spec().revision_id.as_uuid();
        let mut heads = self.heads.write().await;
        if heads.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "KnowledgeBase already exists".into(),
            ));
        }
        let mut revisions = self.revisions.write().await;
        if revisions.contains_key(&revision_id) {
            return Err(RepositoryError::Conflict(
                "KnowledgeBaseRevision already exists".into(),
            ));
        }
        revisions.insert(revision_id, record.revision.clone());
        heads.insert(key, record.clone());
        Ok(record)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
    ) -> Result<Option<KnowledgeBaseRecord>, RepositoryError> {
        Ok(self
            .heads
            .read()
            .await
            .get(&(organization_id, knowledge_base_id))
            .cloned())
    }

    async fn list(&self, limit: usize) -> Result<Vec<KnowledgeBaseRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Ok(self
            .heads
            .read()
            .await
            .values()
            .take(limit)
            .cloned()
            .collect())
    }

    async fn find_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        revision_id: Uuid,
    ) -> Result<Option<KnowledgeBaseRevisionV1>, RepositoryError> {
        let Some(revision) = self.revisions.read().await.get(&revision_id).cloned() else {
            return Ok(None);
        };
        if revision.spec().organization_id.as_uuid() != organization_id
            || revision.spec().knowledge_base_id.as_uuid() != knowledge_base_id
        {
            return Ok(None);
        }
        Ok(Some(revision))
    }

    async fn append_revision(
        &self,
        request: AppendKnowledgeBaseRevision,
    ) -> Result<KnowledgeBaseRecord, RepositoryError> {
        let key = (request.organization_id, request.knowledge_base_id);
        let mut heads = self.heads.write().await;
        let current = heads.get(&key).cloned().ok_or(RepositoryError::NotFound)?;
        let updated = current
            .append(
                request.revision,
                &request.expected_revision_digest,
                request.updated_at,
            )
            .map_err(RepositoryError::Conflict)?;
        let revision_id = updated.revision.spec().revision_id.as_uuid();
        let mut revisions = self.revisions.write().await;
        if revisions.contains_key(&revision_id) {
            return Err(RepositoryError::Conflict(
                "KnowledgeBaseRevision already exists".into(),
            ));
        }
        revisions.insert(revision_id, updated.revision.clone());
        heads.insert(key, updated.clone());
        Ok(updated)
    }
}

/// Deterministic local adapter for the KnowledgePipeline release catalog.
#[derive(Default)]
pub struct InMemoryKnowledgePipelineRepository {
    heads: RwLock<BTreeMap<(Uuid, Uuid), KnowledgePipelineRecord>>,
    releases: RwLock<BTreeMap<Uuid, KnowledgePipelineReleaseV1>>,
}

impl InMemoryKnowledgePipelineRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IKnowledgePipelineRepository for InMemoryKnowledgePipelineRepository {
    async fn create(
        &self,
        request: CreateKnowledgePipeline,
    ) -> Result<KnowledgePipelineRecord, RepositoryError> {
        let record = KnowledgePipelineRecord::new(request.release, request.created_at)
            .map_err(RepositoryError::Conflict)?;
        let key = (
            record.release.spec().organization_id.as_uuid(),
            record.release.spec().pipeline_id.as_uuid(),
        );
        let release_id = record.release.spec().release_id.as_uuid();
        let mut heads = self.heads.write().await;
        if heads.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "KnowledgePipeline already exists".into(),
            ));
        }
        let mut releases = self.releases.write().await;
        if releases.contains_key(&release_id) {
            return Err(RepositoryError::Conflict(
                "KnowledgePipelineRelease already exists".into(),
            ));
        }
        releases.insert(release_id, record.release.clone());
        heads.insert(key, record.clone());
        Ok(record)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        pipeline_id: Uuid,
    ) -> Result<Option<KnowledgePipelineRecord>, RepositoryError> {
        Ok(self
            .heads
            .read()
            .await
            .get(&(organization_id, pipeline_id))
            .cloned())
    }

    async fn find_release(
        &self,
        organization_id: Uuid,
        pipeline_id: Uuid,
        release_id: Uuid,
    ) -> Result<Option<KnowledgePipelineReleaseV1>, RepositoryError> {
        let Some(release) = self.releases.read().await.get(&release_id).cloned() else {
            return Ok(None);
        };
        if release.spec().organization_id.as_uuid() != organization_id
            || release.spec().pipeline_id.as_uuid() != pipeline_id
        {
            return Ok(None);
        }
        Ok(Some(release))
    }

    async fn publish_release(
        &self,
        request: PublishKnowledgePipelineRelease,
    ) -> Result<KnowledgePipelineRecord, RepositoryError> {
        let key = (request.organization_id, request.pipeline_id);
        let mut heads = self.heads.write().await;
        let current = heads.get(&key).cloned().ok_or(RepositoryError::NotFound)?;
        let updated = current
            .publish(
                request.release,
                &request.expected_release_digest,
                request.updated_at,
            )
            .map_err(RepositoryError::Conflict)?;
        let release_id = updated.release.spec().release_id.as_uuid();
        let mut releases = self.releases.write().await;
        if releases.contains_key(&release_id) {
            return Err(RepositoryError::Conflict(
                "KnowledgePipelineRelease already exists".into(),
            ));
        }
        releases.insert(release_id, updated.release.clone());
        heads.insert(key, updated.clone());
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    const BASE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-base-revision.acl"
    ));
    const PIPELINE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-pipeline-release.acl"
    ));

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    use chrono::Utc;

    #[tokio::test]
    async fn knowledge_base_catalog_creates_and_fences_append() {
        let repository = InMemoryKnowledgeBaseRepository::new();
        let revision = KnowledgeBaseRevisionV1::parse_acl(BASE).expect("revision");
        repository
            .create(CreateKnowledgeBase {
                revision: revision.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(repository.list(1).await.expect("list").len(), 1);

        let mut next = revision.spec().clone();
        next.generation = 2;
        next.revision_id = crate::modules::shared_kernel::domain::KnowledgeBaseRevisionId::from_uuid(
            Uuid::from_u128(0x018f0000000070008000000000000303),
        );
        next.name = "Product FAQ v2".into();
        let successor = KnowledgeBaseRevisionV1::from_spec(next).expect("successor");
        let append = AppendKnowledgeBaseRevision {
            organization_id: revision.spec().organization_id.as_uuid(),
            knowledge_base_id: revision.spec().knowledge_base_id.as_uuid(),
            expected_revision_digest: revision.digest().as_str().to_string(),
            revision: successor.clone(),
            updated_at: timestamp(1_001),
        };
        let updated = repository
            .append_revision(append.clone())
            .await
            .expect("append");
        assert_eq!(updated.revision, successor);
        let conflict = repository.append_revision(append).await;
        assert!(matches!(conflict, Err(RepositoryError::Conflict(_))));
    }

    #[tokio::test]
    async fn knowledge_pipeline_catalog_publishes_new_release() {
        let repository = InMemoryKnowledgePipelineRepository::new();
        let release = KnowledgePipelineReleaseV1::parse_acl(PIPELINE).expect("release");
        let created = repository
            .create(CreateKnowledgePipeline {
                release: release.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(
            created.release.spec().pipeline_id,
            release.spec().pipeline_id
        );

        let mut next = release.spec().clone();
        next.release_id =
            crate::modules::shared_kernel::domain::KnowledgePipelineReleaseId::from_uuid(
                Uuid::from_u128(0x018f0000000070008000000000000310),
            );
        next.name = "FAQ ingest v2".into();
        let successor = KnowledgePipelineReleaseV1::from_spec(next).expect("successor");
        let published = repository
            .publish_release(PublishKnowledgePipelineRelease {
                organization_id: release.spec().organization_id.as_uuid(),
                pipeline_id: release.spec().pipeline_id.as_uuid(),
                expected_release_digest: release.digest().as_str().to_string(),
                release: successor.clone(),
                updated_at: timestamp(1_001),
            })
            .await
            .expect("publish");
        assert_eq!(published.release, successor);
    }
}
