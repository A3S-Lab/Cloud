use crate::modules::knowledge::domain::{
    CreateKnowledgeChunk, CreateKnowledgeDocument, IKnowledgeChunkRepository,
    IKnowledgeDocumentRepository, KnowledgeChunkRecord, KnowledgeDocumentRecord,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use std::sync::Arc;
use uuid::Uuid;

/// Application owner boundary for immutable KnowledgeDocument records.
///
/// Presentation composition uses this service instead of reaching into a
/// persistence adapter. Documents are create-once; no mutable update path exists.
#[derive(Clone)]
pub struct KnowledgeDocumentCatalogService {
    repository: Arc<dyn IKnowledgeDocumentRepository>,
}

impl KnowledgeDocumentCatalogService {
    pub fn new(repository: Arc<dyn IKnowledgeDocumentRepository>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        request: CreateKnowledgeDocument,
    ) -> ApplicationResult<KnowledgeDocumentRecord> {
        self.repository.create(request).await.map_err(Into::into)
    }

    pub async fn get(
        &self,
        organization_id: Uuid,
        document_id: Uuid,
    ) -> ApplicationResult<Option<KnowledgeDocumentRecord>> {
        self.repository
            .find(organization_id, document_id)
            .await
            .map_err(Into::into)
    }

    pub async fn list_for_knowledge_base(
        &self,
        organization_id: Uuid,
        knowledge_base_id: Uuid,
        limit: usize,
    ) -> ApplicationResult<Vec<KnowledgeDocumentRecord>> {
        self.repository
            .list_for_knowledge_base(organization_id, knowledge_base_id, limit)
            .await
            .map_err(Into::into)
    }
}

/// Application owner boundary for immutable KnowledgeChunk records.
#[derive(Clone)]
pub struct KnowledgeChunkCatalogService {
    repository: Arc<dyn IKnowledgeChunkRepository>,
}

impl KnowledgeChunkCatalogService {
    pub fn new(repository: Arc<dyn IKnowledgeChunkRepository>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        request: CreateKnowledgeChunk,
    ) -> ApplicationResult<KnowledgeChunkRecord> {
        self.repository.create(request).await.map_err(Into::into)
    }

    pub async fn get(
        &self,
        organization_id: Uuid,
        chunk_id: Uuid,
    ) -> ApplicationResult<Option<KnowledgeChunkRecord>> {
        self.repository
            .find(organization_id, chunk_id)
            .await
            .map_err(Into::into)
    }

    pub async fn list_for_document(
        &self,
        organization_id: Uuid,
        document_id: Uuid,
        limit: usize,
    ) -> ApplicationResult<Vec<KnowledgeChunkRecord>> {
        self.repository
            .list_for_document(organization_id, document_id, limit)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::knowledge::domain::{KnowledgeChunkV1, KnowledgeDocumentV1};
    use crate::modules::knowledge::infrastructure::{
        InMemoryKnowledgeChunkRepository, InMemoryKnowledgeDocumentRepository,
    };
    use chrono::DateTime;

    const DOCUMENT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-document.acl"
    ));
    const CHUNK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-chunk.acl"
    ));

    fn timestamp(seconds: i64) -> DateTime<chrono::Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    #[tokio::test]
    async fn knowledge_document_owner_port_creates_and_discovers() {
        let document = KnowledgeDocumentV1::parse_acl(DOCUMENT).expect("document");
        let service = KnowledgeDocumentCatalogService::new(Arc::new(
            InMemoryKnowledgeDocumentRepository::new(),
        ));
        let created = service
            .create(CreateKnowledgeDocument {
                document: document.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(created.document, document);
        assert_eq!(
            service
                .get(
                    document.spec().organization_id.as_uuid(),
                    document.spec().document_id.as_uuid(),
                )
                .await
                .expect("get")
                .expect("record")
                .document,
            document
        );
        assert_eq!(
            service
                .list_for_knowledge_base(
                    document.spec().organization_id.as_uuid(),
                    document.spec().knowledge_base_id.as_uuid(),
                    8,
                )
                .await
                .expect("list")
                .len(),
            1
        );
        let conflict = service
            .create(CreateKnowledgeDocument {
                document,
                created_at: timestamp(1_001),
            })
            .await;
        assert!(conflict.is_err());
    }

    #[tokio::test]
    async fn knowledge_chunk_owner_port_creates_and_lists_by_document() {
        let chunk = KnowledgeChunkV1::parse_acl(CHUNK).expect("chunk");
        let service =
            KnowledgeChunkCatalogService::new(Arc::new(InMemoryKnowledgeChunkRepository::new()));
        let created = service
            .create(CreateKnowledgeChunk {
                chunk: chunk.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("create");
        assert_eq!(created.chunk, chunk);
        assert_eq!(
            service
                .list_for_document(
                    chunk.spec().organization_id.as_uuid(),
                    chunk.spec().document_id.as_uuid(),
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
                    chunk.spec().organization_id.as_uuid(),
                    chunk.spec().chunk_id.as_uuid(),
                )
                .await
                .expect("get")
                .expect("record")
                .chunk,
            chunk
        );
    }
}
