use crate::modules::knowledge::domain::{
    AppendKnowledgeBaseRevision, CreateKnowledgeBase, CreateKnowledgePipeline,
    IKnowledgeBaseRepository, IKnowledgePipelineRepository, KnowledgeBaseRecord,
    KnowledgeBaseRevisionV1, KnowledgePipelineRecord, KnowledgePipelineReleaseV1,
    PublishKnowledgePipelineRelease,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use std::sync::Arc;
use uuid::Uuid;

/// Application owner boundary for immutable KnowledgeBase revisions.
///
/// Presentation composition uses this service instead of reaching into a
/// persistence adapter. Every change is an exact, digest-fenced revision
/// append; no mutable update path exists.
#[derive(Clone)]
pub struct KnowledgeBaseCatalogService {
    repository: Arc<dyn IKnowledgeBaseRepository>,
}

impl KnowledgeBaseCatalogService {
    pub fn new(repository: Arc<dyn IKnowledgeBaseRepository>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        request: CreateKnowledgeBase,
    ) -> ApplicationResult<KnowledgeBaseRecord> {
        self.repository.create(request).await.map_err(Into::into)
    }

    pub async fn get(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
    ) -> ApplicationResult<Option<KnowledgeBaseRecord>> {
        self.repository
            .find(organization_id, knowledge_base_id)
            .await
            .map_err(Into::into)
    }

    pub async fn list(&self, limit: usize) -> ApplicationResult<Vec<KnowledgeBaseRecord>> {
        self.repository.list(limit).await.map_err(Into::into)
    }

    pub async fn find_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        revision_id: Uuid,
    ) -> ApplicationResult<Option<KnowledgeBaseRevisionV1>> {
        self.repository
            .find_revision(organization_id, knowledge_base_id, revision_id)
            .await
            .map_err(Into::into)
    }

    pub async fn append_revision(
        &self,
        request: AppendKnowledgeBaseRevision,
    ) -> ApplicationResult<KnowledgeBaseRecord> {
        self.repository
            .append_revision(request)
            .await
            .map_err(Into::into)
    }
}

/// Application owner boundary for immutable KnowledgePipeline releases.
#[derive(Clone)]
pub struct KnowledgePipelineCatalogService {
    repository: Arc<dyn IKnowledgePipelineRepository>,
}

impl KnowledgePipelineCatalogService {
    pub fn new(repository: Arc<dyn IKnowledgePipelineRepository>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        request: CreateKnowledgePipeline,
    ) -> ApplicationResult<KnowledgePipelineRecord> {
        self.repository.create(request).await.map_err(Into::into)
    }

    pub async fn get(
        &self,
        organization_id: Uuid,
        pipeline_id: Uuid,
    ) -> ApplicationResult<Option<KnowledgePipelineRecord>> {
        self.repository
            .find(organization_id, pipeline_id)
            .await
            .map_err(Into::into)
    }

    pub async fn find_release(
        &self,
        organization_id: Uuid,
        pipeline_id: Uuid,
        release_id: Uuid,
    ) -> ApplicationResult<Option<KnowledgePipelineReleaseV1>> {
        self.repository
            .find_release(organization_id, pipeline_id, release_id)
            .await
            .map_err(Into::into)
    }

    pub async fn publish_release(
        &self,
        request: PublishKnowledgePipelineRelease,
    ) -> ApplicationResult<KnowledgePipelineRecord> {
        self.repository
            .publish_release(request)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::knowledge::infrastructure::{
        InMemoryKnowledgeBaseRepository, InMemoryKnowledgePipelineRepository,
    };
    use crate::modules::shared_kernel::domain::{
        KnowledgeBaseRevisionId, KnowledgePipelineReleaseId,
    };
    use chrono::DateTime;

    const BASE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-base-revision.acl"
    ));
    const PIPELINE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-pipeline-release.acl"
    ));

    fn timestamp(seconds: i64) -> DateTime<chrono::Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    #[tokio::test]
    async fn knowledge_base_owner_port_creates_discovers_and_appends() {
        let revision = KnowledgeBaseRevisionV1::parse_acl(BASE).expect("revision");
        let service = KnowledgeBaseCatalogService::new(Arc::new(
            InMemoryKnowledgeBaseRepository::new(),
        ));
        let created = service
            .create(CreateKnowledgeBase {
                revision: revision.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(service.list(10).await.expect("list").len(), 1);
        assert_eq!(
            service
                .get(
                    created.revision.spec().organization_id.as_uuid(),
                    created.revision.spec().knowledge_base_id.as_uuid(),
                )
                .await
                .expect("get")
                .expect("record")
                .revision,
            revision
        );

        let mut next = revision.spec().clone();
        next.generation = 2;
        next.revision_id =
            KnowledgeBaseRevisionId::from_uuid(Uuid::from_u128(0x018f0000000070008000000000000303));
        next.name = "Product FAQ v2".into();
        let successor = KnowledgeBaseRevisionV1::from_spec(next).expect("successor");
        let updated = service
            .append_revision(AppendKnowledgeBaseRevision {
                organization_id: revision.spec().organization_id.as_uuid(),
                knowledge_base_id: revision.spec().knowledge_base_id.as_uuid(),
                expected_revision_digest: revision.digest().as_str().to_string(),
                revision: successor.clone(),
                updated_at: timestamp(1_001),
            })
            .await
            .expect("append");
        assert_eq!(updated.revision, successor);
        assert_eq!(
            service
                .find_revision(
                    revision.spec().organization_id.as_uuid(),
                    revision.spec().knowledge_base_id.as_uuid(),
                    revision.spec().revision_id.as_uuid(),
                )
                .await
                .expect("historical")
                .expect("revision"),
            revision
        );
    }

    #[tokio::test]
    async fn knowledge_pipeline_owner_port_creates_and_publishes() {
        let release = KnowledgePipelineReleaseV1::parse_acl(PIPELINE).expect("release");
        let service = KnowledgePipelineCatalogService::new(Arc::new(
            InMemoryKnowledgePipelineRepository::new(),
        ));
        let created = service
            .create(CreateKnowledgePipeline {
                release: release.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(created.release, release);
        let mut next = release.spec().clone();
        next.release_id = KnowledgePipelineReleaseId::from_uuid(Uuid::from_u128(
            0x018f0000000070008000000000000310,
        ));
        next.name = "FAQ ingest v2".into();
        let successor = KnowledgePipelineReleaseV1::from_spec(next).expect("successor");
        let published = service
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
        assert_eq!(
            service
                .find_release(
                    release.spec().organization_id.as_uuid(),
                    release.spec().pipeline_id.as_uuid(),
                    release.spec().release_id.as_uuid(),
                )
                .await
                .expect("historical")
                .expect("release"),
            release
        );
    }
}
