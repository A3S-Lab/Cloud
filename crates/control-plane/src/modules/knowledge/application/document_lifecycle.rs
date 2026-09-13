use super::lifecycle::KnowledgeMutationResult;
use super::resource_access::{knowledge_not_found, project, KnowledgeAccess};
use crate::modules::knowledge::domain::{
    CreateKnowledgeChunkWrite, CreateKnowledgeDocumentWrite, IKnowledgeChunkRepository,
    IKnowledgeDocumentRepository, KnowledgeChunkLifecycleChanged, KnowledgeChunkRecord,
    KnowledgeChunkV1, KnowledgeDocumentLifecycleChanged, KnowledgeDocumentRecord,
    KnowledgeDocumentV1,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateKnowledgeDocumentCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub document_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CreateKnowledgeChunkCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub document_id: Uuid,
    pub chunk_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: KnowledgeAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct GetKnowledgeDocument {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub document_id: Uuid,
    pub access: KnowledgeAccess,
}

#[derive(Debug, Clone)]
pub struct GetKnowledgeChunk {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub chunk_id: Uuid,
    pub access: KnowledgeAccess,
}

/// Authorized KnowledgeDocument/Chunk lifecycle boundary.
///
/// Authorization precedes replay. Creates are exact digest-fenced writes with
/// shared idempotency, audit, and Outbox side effects owned by the repository
/// adapters. Authorized reads share the same KnowledgeAccess projection.
#[derive(Clone)]
pub struct KnowledgeDocumentLifecycleService {
    documents: Arc<dyn IKnowledgeDocumentRepository>,
    chunks: Arc<dyn IKnowledgeChunkRepository>,
}

impl KnowledgeDocumentLifecycleService {
    pub fn new(
        documents: Arc<dyn IKnowledgeDocumentRepository>,
        chunks: Arc<dyn IKnowledgeChunkRepository>,
    ) -> Self {
        Self { documents, chunks }
    }

    pub async fn create_document(
        &self,
        command: CreateKnowledgeDocumentCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<KnowledgeDocumentRecord>> {
        project(command.project_id, &command.access)?;
        let document = KnowledgeDocumentV1::parse_acl(&command.document_acl)
            .map_err(ApplicationError::Invalid)?;
        let spec = document.spec();
        if spec.organization_id != command.organization_id || spec.project_id != command.project_id
        {
            return Err(ApplicationError::Invalid(
                "KnowledgeDocument ACL is outside the requested tenant scope".into(),
            ));
        }
        let canonical = serde_json::to_vec(&CanonicalCreateKnowledgeDocument {
            organization_id: command.organization_id,
            project_id: command.project_id,
            document_id: spec.document_id.as_uuid(),
            document_digest: document.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/knowledge-documents",
                command.organization_id, command.project_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.documents.replay_write(&idempotency).await? {
            if !create_document_replay_matches(&record, &command, &document) {
                return Err(ApplicationError::Internal(
                    "KnowledgeDocument create replay reference is inconsistent".into(),
                ));
            }
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record =
            KnowledgeDocumentRecord::new(document, now).map_err(ApplicationError::Invalid)?;
        let event = KnowledgeDocumentLifecycleChanged::created(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .documents
            .create_write(CreateKnowledgeDocumentWrite {
                record,
                event,
                actor_principal_id: command.actor_principal_id,
                request_id: command.request_id,
                idempotency,
            })
            .await?;
        Ok(KnowledgeMutationResult {
            record: written.value,
            replayed: written.replayed,
        })
    }

    pub async fn create_chunk(
        &self,
        command: CreateKnowledgeChunkCommand,
    ) -> ApplicationResult<KnowledgeMutationResult<KnowledgeChunkRecord>> {
        project(command.project_id, &command.access)?;
        let chunk =
            KnowledgeChunkV1::parse_acl(&command.chunk_acl).map_err(ApplicationError::Invalid)?;
        let spec = chunk.spec();
        if spec.organization_id != command.organization_id
            || spec.project_id != command.project_id
            || spec.document_id.as_uuid() != command.document_id
        {
            return Err(ApplicationError::Invalid(
                "KnowledgeChunk ACL is outside the requested tenant scope".into(),
            ));
        }
        let canonical = serde_json::to_vec(&CanonicalCreateKnowledgeChunk {
            organization_id: command.organization_id,
            project_id: command.project_id,
            document_id: command.document_id,
            chunk_id: spec.chunk_id.as_uuid(),
            chunk_digest: chunk.digest().as_str(),
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/knowledge-documents/{}/chunks",
                command.organization_id, command.project_id, command.document_id
            ),
            command.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.chunks.replay_write(&idempotency).await? {
            if !create_chunk_replay_matches(&record, &command, &chunk) {
                return Err(ApplicationError::Internal(
                    "KnowledgeChunk create replay reference is inconsistent".into(),
                ));
            }
            return Ok(KnowledgeMutationResult {
                record,
                replayed: true,
            });
        }
        let now = canonical_now()?;
        let record = KnowledgeChunkRecord::new(chunk, now).map_err(ApplicationError::Invalid)?;
        let event = KnowledgeChunkLifecycleChanged::created(&record, command.request_id)
            .map_err(ApplicationError::Internal)?;
        let written = self
            .chunks
            .create_write(CreateKnowledgeChunkWrite {
                record,
                event,
                actor_principal_id: command.actor_principal_id,
                request_id: command.request_id,
                idempotency,
            })
            .await?;
        Ok(KnowledgeMutationResult {
            record: written.value,
            replayed: written.replayed,
        })
    }

    pub async fn get_document(
        &self,
        query: GetKnowledgeDocument,
    ) -> ApplicationResult<KnowledgeDocumentRecord> {
        project(query.project_id, &query.access)?;
        let record = self
            .documents
            .find(query.organization_id.as_uuid(), query.document_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if record.document.spec().project_id != query.project_id {
            return Err(knowledge_not_found());
        }
        Ok(record)
    }

    pub async fn get_chunk(
        &self,
        query: GetKnowledgeChunk,
    ) -> ApplicationResult<KnowledgeChunkRecord> {
        project(query.project_id, &query.access)?;
        let record = self
            .chunks
            .find(query.organization_id.as_uuid(), query.chunk_id)
            .await?
            .ok_or_else(knowledge_not_found)?;
        if record.chunk.spec().project_id != query.project_id {
            return Err(knowledge_not_found());
        }
        Ok(record)
    }
}

fn create_document_replay_matches(
    record: &KnowledgeDocumentRecord,
    command: &CreateKnowledgeDocumentCommand,
    document: &KnowledgeDocumentV1,
) -> bool {
    let spec = record.document.spec();
    spec.organization_id == command.organization_id
        && spec.project_id == command.project_id
        && record.document.digest().as_str() == document.digest().as_str()
}

fn create_chunk_replay_matches(
    record: &KnowledgeChunkRecord,
    command: &CreateKnowledgeChunkCommand,
    chunk: &KnowledgeChunkV1,
) -> bool {
    let spec = record.chunk.spec();
    spec.organization_id == command.organization_id
        && spec.project_id == command.project_id
        && spec.document_id.as_uuid() == command.document_id
        && record.chunk.digest().as_str() == chunk.digest().as_str()
}

fn canonical_now() -> ApplicationResult<chrono::DateTime<Utc>> {
    let now = Utc::now();
    truncate_to_seconds(now).ok_or_else(|| {
        ApplicationError::Internal("Knowledge catalog timestamps require whole seconds".into())
    })
}

fn truncate_to_seconds(value: chrono::DateTime<Utc>) -> Option<chrono::DateTime<Utc>> {
    chrono::DateTime::from_timestamp(value.timestamp(), 0)
}

#[derive(Serialize)]
struct CanonicalCreateKnowledgeDocument<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    document_id: Uuid,
    document_digest: &'a str,
}

#[derive(Serialize)]
struct CanonicalCreateKnowledgeChunk<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    document_id: Uuid,
    chunk_id: Uuid,
    chunk_digest: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::knowledge::infrastructure::{
        InMemoryKnowledgeChunkRepository, InMemoryKnowledgeDocumentRepository,
    };

    const DOCUMENT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-document.acl"
    ));
    const CHUNK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/k0.1/knowledge-chunk.acl"
    ));

    #[tokio::test]
    async fn authorized_document_create_replays_exactly_once() {
        let documents = Arc::new(InMemoryKnowledgeDocumentRepository::new());
        let chunks = Arc::new(InMemoryKnowledgeChunkRepository::new());
        let service = KnowledgeDocumentLifecycleService::new(documents, chunks);
        let document = KnowledgeDocumentV1::parse_acl(DOCUMENT).expect("document");
        let command = CreateKnowledgeDocumentCommand {
            organization_id: document.spec().organization_id,
            project_id: document.spec().project_id,
            document_acl: DOCUMENT.to_owned(),
            actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
            access: KnowledgeAccess::restricted_projects([document.spec().project_id]),
            idempotency_key: "create-doc-1".into(),
            request_id: Uuid::from_u128(0x88),
        };
        let first = service
            .create_document(command.clone())
            .await
            .expect("create");
        assert!(!first.replayed);
        let second = service.create_document(command).await.expect("replay");
        assert!(second.replayed);
        assert_eq!(first.record.document, second.record.document);

        let denied = service
            .create_document(CreateKnowledgeDocumentCommand {
                organization_id: document.spec().organization_id,
                project_id: document.spec().project_id,
                document_acl: DOCUMENT.to_owned(),
                actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
                access: KnowledgeAccess::restricted_projects([ProjectId::new()]),
                idempotency_key: "create-doc-2".into(),
                request_id: Uuid::from_u128(0x89),
            })
            .await;
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    }

    #[tokio::test]
    async fn authorized_chunk_create_replays_exactly_once() {
        let documents = Arc::new(InMemoryKnowledgeDocumentRepository::new());
        let chunks = Arc::new(InMemoryKnowledgeChunkRepository::new());
        let service = KnowledgeDocumentLifecycleService::new(documents, chunks);
        let chunk = KnowledgeChunkV1::parse_acl(CHUNK).expect("chunk");
        let command = CreateKnowledgeChunkCommand {
            organization_id: chunk.spec().organization_id,
            project_id: chunk.spec().project_id,
            document_id: chunk.spec().document_id.as_uuid(),
            chunk_acl: CHUNK.to_owned(),
            actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
            access: KnowledgeAccess::restricted_projects([chunk.spec().project_id]),
            idempotency_key: "create-chunk-1".into(),
            request_id: Uuid::from_u128(0x8a),
        };
        let first = service.create_chunk(command.clone()).await.expect("create");
        assert!(!first.replayed);
        let second = service.create_chunk(command).await.expect("replay");
        assert!(second.replayed);
        assert_eq!(first.record.chunk, second.record.chunk);

        let denied = service
            .create_chunk(CreateKnowledgeChunkCommand {
                organization_id: chunk.spec().organization_id,
                project_id: chunk.spec().project_id,
                document_id: chunk.spec().document_id.as_uuid(),
                chunk_acl: CHUNK.to_owned(),
                actor_principal_id: PrincipalId::from_uuid(Uuid::from_u128(0x77)),
                access: KnowledgeAccess::restricted_projects([ProjectId::new()]),
                idempotency_key: "create-chunk-2".into(),
                request_id: Uuid::from_u128(0x8b),
            })
            .await;
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    }
}
