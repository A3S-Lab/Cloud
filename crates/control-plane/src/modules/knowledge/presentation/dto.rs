use crate::modules::knowledge::application::KnowledgeMutationResult;
use crate::modules::knowledge::domain::{
    ExternalKnowledgeBindingRecord, KnowledgeBaseRecord, KnowledgeChunkRecord,
    KnowledgeDocumentRecord, KnowledgeIndexRevisionRecord, KnowledgePipelineRecord,
    KnowledgeRetrievalPolicyRevisionRecord, EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1,
    KNOWLEDGE_BASE_REVISION_SCHEMA_V1, KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_DOCUMENT_SCHEMA_V1,
    KNOWLEDGE_INDEX_REVISION_SCHEMA_V1, KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeBaseRequest {
    pub revision_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendKnowledgeBaseRequest {
    pub expected_revision_digest: String,
    pub revision_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgePipelineRequest {
    pub release_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishKnowledgePipelineRequest {
    pub expected_release_digest: String,
    pub release_acl: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBaseResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub knowledge_base_id: Uuid,
    pub revision_id: Uuid,
    pub generation: u64,
    pub name: String,
    pub contract_schema: String,
    pub revision_acl: String,
    pub revision_digest: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<KnowledgeBaseRecord> for KnowledgeBaseResponse {
    fn from(record: KnowledgeBaseRecord) -> Self {
        let spec = record.revision.spec();
        Self {
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            knowledge_base_id: spec.knowledge_base_id.as_uuid(),
            revision_id: spec.revision_id.as_uuid(),
            generation: spec.generation,
            name: spec.name.clone(),
            contract_schema: KNOWLEDGE_BASE_REVISION_SCHEMA_V1.into(),
            revision_acl: record.revision.canonical_acl().to_owned(),
            revision_digest: record.revision.digest().as_str().into(),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBaseMutationResponse {
    pub knowledge_base: KnowledgeBaseResponse,
    pub replayed: bool,
}

impl From<KnowledgeMutationResult<KnowledgeBaseRecord>> for KnowledgeBaseMutationResponse {
    fn from(result: KnowledgeMutationResult<KnowledgeBaseRecord>) -> Self {
        Self {
            knowledge_base: result.record.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgePipelineResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub pipeline_id: Uuid,
    pub release_id: Uuid,
    pub name: String,
    pub contract_schema: String,
    pub release_acl: String,
    pub release_digest: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<KnowledgePipelineRecord> for KnowledgePipelineResponse {
    fn from(record: KnowledgePipelineRecord) -> Self {
        let spec = record.release.spec();
        Self {
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            pipeline_id: spec.pipeline_id.as_uuid(),
            release_id: spec.release_id.as_uuid(),
            name: spec.name.clone(),
            contract_schema: KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1.into(),
            release_acl: record.release.canonical_acl().to_owned(),
            release_digest: record.release.digest().as_str().into(),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgePipelineMutationResponse {
    pub knowledge_pipeline: KnowledgePipelineResponse,
    pub replayed: bool,
}

impl From<KnowledgeMutationResult<KnowledgePipelineRecord>> for KnowledgePipelineMutationResponse {
    fn from(result: KnowledgeMutationResult<KnowledgePipelineRecord>) -> Self {
        Self {
            knowledge_pipeline: result.record.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeDocumentRequest {
    pub document_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeChunkRequest {
    pub chunk_acl: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDocumentResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub knowledge_base_id: Uuid,
    pub knowledge_base_revision_id: Uuid,
    pub title: String,
    pub contract_schema: String,
    pub document_acl: String,
    pub document_digest: String,
    pub created_at: DateTime<Utc>,
}

impl From<KnowledgeDocumentRecord> for KnowledgeDocumentResponse {
    fn from(record: KnowledgeDocumentRecord) -> Self {
        let spec = record.document.spec();
        Self {
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            document_id: spec.document_id.as_uuid(),
            knowledge_base_id: spec.knowledge_base_id.as_uuid(),
            knowledge_base_revision_id: spec.knowledge_base_revision_id.as_uuid(),
            title: spec.title.clone(),
            contract_schema: KNOWLEDGE_DOCUMENT_SCHEMA_V1.into(),
            document_acl: record.document.canonical_acl().to_owned(),
            document_digest: record.document.digest().as_str().into(),
            created_at: record.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDocumentMutationResponse {
    pub knowledge_document: KnowledgeDocumentResponse,
    pub replayed: bool,
}

impl From<KnowledgeMutationResult<KnowledgeDocumentRecord>> for KnowledgeDocumentMutationResponse {
    fn from(result: KnowledgeMutationResult<KnowledgeDocumentRecord>) -> Self {
        Self {
            knowledge_document: result.record.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeChunkResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub chunk_id: Uuid,
    pub ordinal: u64,
    pub contract_schema: String,
    pub chunk_acl: String,
    pub chunk_digest: String,
    pub created_at: DateTime<Utc>,
}

impl From<KnowledgeChunkRecord> for KnowledgeChunkResponse {
    fn from(record: KnowledgeChunkRecord) -> Self {
        let spec = record.chunk.spec();
        Self {
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            document_id: spec.document_id.as_uuid(),
            chunk_id: spec.chunk_id.as_uuid(),
            ordinal: spec.ordinal,
            contract_schema: KNOWLEDGE_CHUNK_SCHEMA_V1.into(),
            chunk_acl: record.chunk.canonical_acl().to_owned(),
            chunk_digest: record.chunk.digest().as_str().into(),
            created_at: record.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeChunkMutationResponse {
    pub knowledge_chunk: KnowledgeChunkResponse,
    pub replayed: bool,
}

impl From<KnowledgeMutationResult<KnowledgeChunkRecord>> for KnowledgeChunkMutationResponse {
    fn from(result: KnowledgeMutationResult<KnowledgeChunkRecord>) -> Self {
        Self {
            knowledge_chunk: result.record.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeIndexRevisionRequest {
    pub index_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeRetrievalPolicyRevisionRequest {
    pub policy_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateExternalKnowledgeBindingRequest {
    pub binding_acl: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeIndexRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub knowledge_base_revision_id: Uuid,
    pub index_revision_id: Uuid,
    pub strategy: String,
    pub embedding_dimension: u32,
    pub contract_schema: String,
    pub index_acl: String,
    pub index_digest: String,
    pub created_at: DateTime<Utc>,
}

impl From<KnowledgeIndexRevisionRecord> for KnowledgeIndexRevisionResponse {
    fn from(record: KnowledgeIndexRevisionRecord) -> Self {
        let spec = record.index_revision.spec();
        Self {
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            knowledge_base_revision_id: spec.knowledge_base_revision_id.as_uuid(),
            index_revision_id: spec.index_revision_id.as_uuid(),
            strategy: spec.strategy.as_str().into(),
            embedding_dimension: spec.embedding_dimension,
            contract_schema: KNOWLEDGE_INDEX_REVISION_SCHEMA_V1.into(),
            index_acl: record.index_revision.canonical_acl().to_owned(),
            index_digest: record.index_revision.digest().as_str().into(),
            created_at: record.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeIndexRevisionMutationResponse {
    pub knowledge_index_revision: KnowledgeIndexRevisionResponse,
    pub replayed: bool,
}

impl From<KnowledgeMutationResult<KnowledgeIndexRevisionRecord>>
    for KnowledgeIndexRevisionMutationResponse
{
    fn from(result: KnowledgeMutationResult<KnowledgeIndexRevisionRecord>) -> Self {
        Self {
            knowledge_index_revision: result.record.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRetrievalPolicyRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub knowledge_base_revision_id: Uuid,
    pub policy_revision_id: Uuid,
    pub search_mode: String,
    pub top_k: u32,
    pub contract_schema: String,
    pub policy_acl: String,
    pub policy_digest: String,
    pub created_at: DateTime<Utc>,
}

impl From<KnowledgeRetrievalPolicyRevisionRecord> for KnowledgeRetrievalPolicyRevisionResponse {
    fn from(record: KnowledgeRetrievalPolicyRevisionRecord) -> Self {
        let spec = record.policy_revision.spec();
        Self {
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            knowledge_base_revision_id: spec.knowledge_base_revision_id.as_uuid(),
            policy_revision_id: spec.policy_revision_id.as_uuid(),
            search_mode: spec.search_mode.clone(),
            top_k: spec.top_k,
            contract_schema: KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1.into(),
            policy_acl: record.policy_revision.canonical_acl().to_owned(),
            policy_digest: record.policy_revision.digest().as_str().into(),
            created_at: record.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRetrievalPolicyRevisionMutationResponse {
    pub knowledge_retrieval_policy_revision: KnowledgeRetrievalPolicyRevisionResponse,
    pub replayed: bool,
}

impl From<KnowledgeMutationResult<KnowledgeRetrievalPolicyRevisionRecord>>
    for KnowledgeRetrievalPolicyRevisionMutationResponse
{
    fn from(result: KnowledgeMutationResult<KnowledgeRetrievalPolicyRevisionRecord>) -> Self {
        Self {
            knowledge_retrieval_policy_revision: result.record.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalKnowledgeBindingResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub knowledge_base_id: Uuid,
    pub binding_id: Uuid,
    pub display_name: String,
    pub contract_schema: String,
    pub binding_acl: String,
    pub binding_digest: String,
    pub created_at: DateTime<Utc>,
}

impl From<ExternalKnowledgeBindingRecord> for ExternalKnowledgeBindingResponse {
    fn from(record: ExternalKnowledgeBindingRecord) -> Self {
        let spec = record.binding.spec();
        Self {
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            knowledge_base_id: spec.knowledge_base_id.as_uuid(),
            binding_id: spec.binding_id.as_uuid(),
            display_name: spec.display_name.clone(),
            contract_schema: EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1.into(),
            binding_acl: record.binding.canonical_acl().to_owned(),
            binding_digest: record.binding.digest().as_str().into(),
            created_at: record.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalKnowledgeBindingMutationResponse {
    pub external_knowledge_binding: ExternalKnowledgeBindingResponse,
    pub replayed: bool,
}

impl From<KnowledgeMutationResult<ExternalKnowledgeBindingRecord>>
    for ExternalKnowledgeBindingMutationResponse
{
    fn from(result: KnowledgeMutationResult<ExternalKnowledgeBindingRecord>) -> Self {
        Self {
            external_knowledge_binding: result.record.into(),
            replayed: result.replayed,
        }
    }
}
