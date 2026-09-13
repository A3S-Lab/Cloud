use crate::modules::knowledge::domain::{
    CreateExternalKnowledgeBinding, CreateKnowledgeIndexRevision,
    CreateKnowledgeRetrievalPolicyRevision, ExternalKnowledgeBindingRecord,
    IExternalKnowledgeBindingRepository, IKnowledgeIndexRevisionRepository,
    IKnowledgeRetrievalPolicyRevisionRepository, KnowledgeIndexRevisionRecord,
    KnowledgeRetrievalPolicyRevisionRecord,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

pub struct InMemoryKnowledgeIndexRevisionRepository {
    records: RwLock<BTreeMap<(Uuid, Uuid), KnowledgeIndexRevisionRecord>>,
}

impl Default for InMemoryKnowledgeIndexRevisionRepository {
    fn default() -> Self {
        Self {
            records: RwLock::new(BTreeMap::new()),
        }
    }
}

impl InMemoryKnowledgeIndexRevisionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IKnowledgeIndexRevisionRepository for InMemoryKnowledgeIndexRevisionRepository {
    async fn create(
        &self,
        request: CreateKnowledgeIndexRevision,
    ) -> Result<KnowledgeIndexRevisionRecord, RepositoryError> {
        let record =
            KnowledgeIndexRevisionRecord::new(request.index_revision, request.created_at)
                .map_err(RepositoryError::Conflict)?;
        let key = (
            record.index_revision.spec().organization_id.as_uuid(),
            record.index_revision.spec().index_revision_id.as_uuid(),
        );
        let mut records = self.records.write().await;
        if records.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "KnowledgeIndexRevision already exists".into(),
            ));
        }
        records.insert(key, record.clone());
        Ok(record)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        index_revision_id: Uuid,
    ) -> Result<Option<KnowledgeIndexRevisionRecord>, RepositoryError> {
        Ok(self
            .records
            .read()
            .await
            .get(&(organization_id, index_revision_id))
            .cloned())
    }

    async fn list_for_knowledge_base_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_revision_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeIndexRevisionRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Ok(self
            .records
            .read()
            .await
            .values()
            .filter(|record| {
                record.index_revision.spec().organization_id.as_uuid() == organization_id
                    && record
                        .index_revision
                        .spec()
                        .knowledge_base_revision_id
                        .as_uuid()
                        == knowledge_base_revision_id
            })
            .take(limit)
            .cloned()
            .collect())
    }
}

pub struct InMemoryKnowledgeRetrievalPolicyRevisionRepository {
    records: RwLock<BTreeMap<(Uuid, Uuid), KnowledgeRetrievalPolicyRevisionRecord>>,
}

impl Default for InMemoryKnowledgeRetrievalPolicyRevisionRepository {
    fn default() -> Self {
        Self {
            records: RwLock::new(BTreeMap::new()),
        }
    }
}

impl InMemoryKnowledgeRetrievalPolicyRevisionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IKnowledgeRetrievalPolicyRevisionRepository
    for InMemoryKnowledgeRetrievalPolicyRevisionRepository
{
    async fn create(
        &self,
        request: CreateKnowledgeRetrievalPolicyRevision,
    ) -> Result<KnowledgeRetrievalPolicyRevisionRecord, RepositoryError> {
        let record = KnowledgeRetrievalPolicyRevisionRecord::new(
            request.policy_revision,
            request.created_at,
        )
        .map_err(RepositoryError::Conflict)?;
        let key = (
            record.policy_revision.spec().organization_id.as_uuid(),
            record.policy_revision.spec().policy_revision_id.as_uuid(),
        );
        let mut records = self.records.write().await;
        if records.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "KnowledgeRetrievalPolicyRevision already exists".into(),
            ));
        }
        records.insert(key, record.clone());
        Ok(record)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        policy_revision_id: Uuid,
    ) -> Result<Option<KnowledgeRetrievalPolicyRevisionRecord>, RepositoryError> {
        Ok(self
            .records
            .read()
            .await
            .get(&(organization_id, policy_revision_id))
            .cloned())
    }

    async fn list_for_knowledge_base_revision(
        &self,
        organization_id: Uuid,
        knowledge_base_revision_id: Uuid,
        limit: usize,
    ) -> Result<Vec<KnowledgeRetrievalPolicyRevisionRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Ok(self
            .records
            .read()
            .await
            .values()
            .filter(|record| {
                record.policy_revision.spec().organization_id.as_uuid() == organization_id
                    && record
                        .policy_revision
                        .spec()
                        .knowledge_base_revision_id
                        .as_uuid()
                        == knowledge_base_revision_id
            })
            .take(limit)
            .cloned()
            .collect())
    }
}

pub struct InMemoryExternalKnowledgeBindingRepository {
    records: RwLock<BTreeMap<(Uuid, Uuid), ExternalKnowledgeBindingRecord>>,
}

impl Default for InMemoryExternalKnowledgeBindingRepository {
    fn default() -> Self {
        Self {
            records: RwLock::new(BTreeMap::new()),
        }
    }
}

impl InMemoryExternalKnowledgeBindingRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IExternalKnowledgeBindingRepository for InMemoryExternalKnowledgeBindingRepository {
    async fn create(
        &self,
        request: CreateExternalKnowledgeBinding,
    ) -> Result<ExternalKnowledgeBindingRecord, RepositoryError> {
        let record = ExternalKnowledgeBindingRecord::new(request.binding, request.created_at)
            .map_err(RepositoryError::Conflict)?;
        let key = (
            record.binding.spec().organization_id.as_uuid(),
            record.binding.spec().binding_id.as_uuid(),
        );
        let mut records = self.records.write().await;
        if records.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "ExternalKnowledgeBinding already exists".into(),
            ));
        }
        records.insert(key, record.clone());
        Ok(record)
    }

    async fn find(
        &self,
        organization_id: Uuid,
        binding_id: Uuid,
    ) -> Result<Option<ExternalKnowledgeBindingRecord>, RepositoryError> {
        Ok(self
            .records
            .read()
            .await
            .get(&(organization_id, binding_id))
            .cloned())
    }

    async fn list_for_knowledge_base(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ExternalKnowledgeBindingRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Ok(self
            .records
            .read()
            .await
            .values()
            .filter(|record| {
                record.binding.spec().organization_id.as_uuid() == organization_id
                    && record.binding.spec().knowledge_base_id.as_uuid() == knowledge_base_id
            })
            .take(limit)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::knowledge::domain::{
        ExternalKnowledgeBindingV1, KnowledgeIndexRevisionV1, KnowledgeRetrievalPolicyRevisionV1,
    };
    use chrono::{TimeZone, Utc};

    const INDEX_FIXTURE: &str =
        include_str!("../../../../../../contracts/k0.1/knowledge-index-revision.acl");
    const POLICY_FIXTURE: &str =
        include_str!("../../../../../../contracts/k0.1/knowledge-retrieval-policy-revision.acl");
    const BINDING_FIXTURE: &str =
        include_str!("../../../../../../contracts/k0.1/external-knowledge-binding.acl");

    fn ts(seconds: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).unwrap()
    }

    #[tokio::test]
    async fn knowledge_index_policy_binding_catalogs_create_find_and_list() {
        let index = KnowledgeIndexRevisionV1::parse_acl(INDEX_FIXTURE).expect("index");
        let indexes = InMemoryKnowledgeIndexRevisionRepository::new();
        let created = indexes
            .create(CreateKnowledgeIndexRevision {
                index_revision: index.clone(),
                created_at: ts(1_000),
            })
            .await
            .expect("create index");
        assert_eq!(created.index_revision, index);
        assert!(indexes
            .create(CreateKnowledgeIndexRevision {
                index_revision: index.clone(),
                created_at: ts(1_001),
            })
            .await
            .is_err());
        assert_eq!(
            indexes
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

        let policy = KnowledgeRetrievalPolicyRevisionV1::parse_acl(POLICY_FIXTURE).expect("policy");
        let policies = InMemoryKnowledgeRetrievalPolicyRevisionRepository::new();
        policies
            .create(CreateKnowledgeRetrievalPolicyRevision {
                policy_revision: policy.clone(),
                created_at: ts(1_010),
            })
            .await
            .expect("create policy");
        assert_eq!(
            policies
                .find(
                    policy.spec().organization_id.as_uuid(),
                    policy.spec().policy_revision_id.as_uuid(),
                )
                .await
                .expect("find")
                .expect("present")
                .policy_revision,
            policy
        );

        let binding = ExternalKnowledgeBindingV1::parse_acl(BINDING_FIXTURE).expect("binding");
        let bindings = InMemoryExternalKnowledgeBindingRepository::new();
        bindings
            .create(CreateExternalKnowledgeBinding {
                binding: binding.clone(),
                created_at: ts(1_020),
            })
            .await
            .expect("create binding");
        assert_eq!(
            bindings
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
    }
}
