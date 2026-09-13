use crate::modules::knowledge::domain::{
    CreateExternalKnowledgeBinding, CreateKnowledgeIndexRevision,
    CreateKnowledgeRetrievalPolicyRevision, ExternalKnowledgeBindingRecord,
    IExternalKnowledgeBindingRepository, IKnowledgeIndexRevisionRepository,
    IKnowledgeRetrievalPolicyRevisionRepository, KnowledgeIndexRevisionRecord,
    KnowledgeRetrievalPolicyRevisionRecord,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use std::sync::Arc;
use uuid::Uuid;

/// Application owner boundary for immutable KnowledgeIndexRevision records.
///
/// Presentation composition uses this service instead of reaching into a
/// persistence adapter. Index revisions are create-once; no mutable update path exists.
#[derive(Clone)]
pub struct KnowledgeIndexRevisionCatalogService {
    repository: Arc<dyn IKnowledgeIndexRevisionRepository>,
}

impl KnowledgeIndexRevisionCatalogService {
    pub fn new(repository: Arc<dyn IKnowledgeIndexRevisionRepository>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        request: CreateKnowledgeIndexRevision,
    ) -> ApplicationResult<KnowledgeIndexRevisionRecord> {
        self.repository.create(request).await.map_err(Into::into)
    }

    pub async fn get(
        &self,
        organization_id: Uuid,
        index_revision_id: Uuid,
    ) -> ApplicationResult<Option<KnowledgeIndexRevisionRecord>> {
        self.repository
            .find(organization_id, index_revision_id)
            .await
            .map_err(Into::into)
    }

    pub async fn list_for_knowledge_base_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_revision_id: Uuid,
        limit: usize,
    ) -> ApplicationResult<Vec<KnowledgeIndexRevisionRecord>> {
        self.repository
            .list_for_knowledge_base_revision(
                organization_id,
                knowledge_base_revision_id,
                limit,
            )
            .await
            .map_err(Into::into)
    }
}

/// Application owner boundary for immutable KnowledgeRetrievalPolicyRevision records.
#[derive(Clone)]
pub struct KnowledgeRetrievalPolicyRevisionCatalogService {
    repository: Arc<dyn IKnowledgeRetrievalPolicyRevisionRepository>,
}

impl KnowledgeRetrievalPolicyRevisionCatalogService {
    pub fn new(repository: Arc<dyn IKnowledgeRetrievalPolicyRevisionRepository>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        request: CreateKnowledgeRetrievalPolicyRevision,
    ) -> ApplicationResult<KnowledgeRetrievalPolicyRevisionRecord> {
        self.repository.create(request).await.map_err(Into::into)
    }

    pub async fn get(
        &self,
        organization_id: Uuid,
        policy_revision_id: Uuid,
    ) -> ApplicationResult<Option<KnowledgeRetrievalPolicyRevisionRecord>> {
        self.repository
            .find(organization_id, policy_revision_id)
            .await
            .map_err(Into::into)
    }

    pub async fn list_for_knowledge_base_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_revision_id: Uuid,
        limit: usize,
    ) -> ApplicationResult<Vec<KnowledgeRetrievalPolicyRevisionRecord>> {
        self.repository
            .list_for_knowledge_base_revision(
                organization_id,
                knowledge_base_revision_id,
                limit,
            )
            .await
            .map_err(Into::into)
    }
}

/// Application owner boundary for immutable ExternalKnowledgeBinding records.
#[derive(Clone)]
pub struct ExternalKnowledgeBindingCatalogService {
    repository: Arc<dyn IExternalKnowledgeBindingRepository>,
}

impl ExternalKnowledgeBindingCatalogService {
    pub fn new(repository: Arc<dyn IExternalKnowledgeBindingRepository>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        request: CreateExternalKnowledgeBinding,
    ) -> ApplicationResult<ExternalKnowledgeBindingRecord> {
        self.repository.create(request).await.map_err(Into::into)
    }

    pub async fn get(
        &self,
        organization_id: Uuid,
        binding_id: Uuid,
    ) -> ApplicationResult<Option<ExternalKnowledgeBindingRecord>> {
        self.repository
            .find(organization_id, binding_id)
            .await
            .map_err(Into::into)
    }

    pub async fn list_for_knowledge_base(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        limit: usize,
    ) -> ApplicationResult<Vec<ExternalKnowledgeBindingRecord>> {
        self.repository
            .list_for_knowledge_base(organization_id, knowledge_base_id, limit)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::knowledge::domain::{
        ExternalKnowledgeBindingV1, KnowledgeIndexRevisionV1, KnowledgeRetrievalPolicyRevisionV1,
    };
    use crate::modules::knowledge::infrastructure::{
        InMemoryExternalKnowledgeBindingRepository, InMemoryKnowledgeIndexRevisionRepository,
        InMemoryKnowledgeRetrievalPolicyRevisionRepository,
    };
    use chrono::DateTime;

    const INDEX: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-index-revision.acl"
    ));
    const POLICY: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-retrieval-policy-revision.acl"
    ));
    const BINDING: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/external-knowledge-binding.acl"
    ));

    fn timestamp(seconds: i64) -> DateTime<chrono::Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    #[tokio::test]
    async fn knowledge_index_owner_port_creates_and_discovers() {
        let index = KnowledgeIndexRevisionV1::parse_acl(INDEX).expect("index");
        let service = KnowledgeIndexRevisionCatalogService::new(Arc::new(
            InMemoryKnowledgeIndexRevisionRepository::new(),
        ));
        let created = service
            .create(CreateKnowledgeIndexRevision {
                index_revision: index.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(created.index_revision, index);
        assert_eq!(
            service
                .get(
                    index.spec().organization_id.as_uuid(),
                    index.spec().index_revision_id.as_uuid(),
                )
                .await
                .expect("get")
                .expect("record")
                .index_revision,
            index
        );
        assert_eq!(
            service
                .list_for_knowledge_base_revision(
                    index.spec().organization_id.as_uuid(),
                    index.spec().knowledge_base_revision_id.as_uuid(),
                    8,
                )
                .await
                .expect("list")
                .len(),
            1
        );
        let conflict = service
            .create(CreateKnowledgeIndexRevision {
                index_revision: index,
                created_at: timestamp(1_001),
            })
            .await;
        assert!(conflict.is_err());
    }

    #[tokio::test]
    async fn knowledge_policy_owner_port_creates_and_lists() {
        let policy = KnowledgeRetrievalPolicyRevisionV1::parse_acl(POLICY).expect("policy");
        let service = KnowledgeRetrievalPolicyRevisionCatalogService::new(Arc::new(
            InMemoryKnowledgeRetrievalPolicyRevisionRepository::new(),
        ));
        let created = service
            .create(CreateKnowledgeRetrievalPolicyRevision {
                policy_revision: policy.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(created.policy_revision, policy);
        assert_eq!(
            service
                .list_for_knowledge_base_revision(
                    policy.spec().organization_id.as_uuid(),
                    policy.spec().knowledge_base_revision_id.as_uuid(),
                    8,
                )
                .await
                .expect("list")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn external_binding_owner_port_creates_and_lists() {
        let binding = ExternalKnowledgeBindingV1::parse_acl(BINDING).expect("binding");
        let service = ExternalKnowledgeBindingCatalogService::new(Arc::new(
            InMemoryExternalKnowledgeBindingRepository::new(),
        ));
        let created = service
            .create(CreateExternalKnowledgeBinding {
                binding: binding.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(created.binding, binding);
        assert_eq!(
            service
                .list_for_knowledge_base(
                    binding.spec().organization_id.as_uuid(),
                    binding.spec().knowledge_base_id.as_uuid(),
                    8,
                )
                .await
                .expect("list")
                .len(),
            1
        );
        assert_eq!(
            service
                .get(
                    binding.spec().organization_id.as_uuid(),
                    binding.spec().binding_id.as_uuid(),
                )
                .await
                .expect("get")
                .expect("record")
                .binding,
            binding
        );
    }
}
