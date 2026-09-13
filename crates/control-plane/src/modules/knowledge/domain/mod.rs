mod acl;
mod base_revision;
mod catalog;
mod chunk;
mod document;
mod document_catalog;
mod external_binding;
mod index_revision;
mod pipeline_release;
mod retrieval_policy;
mod types;
mod writes;

#[cfg(test)]
mod tests;

pub use base_revision::{KnowledgeBaseRevisionSpecV1, KnowledgeBaseRevisionV1};
pub use catalog::{
    AppendKnowledgeBaseRevision, CreateKnowledgeBase, CreateKnowledgePipeline,
    IKnowledgeBaseRepository, IKnowledgePipelineRepository, KnowledgeBaseRecord,
    KnowledgePipelineRecord, PublishKnowledgePipelineRelease,
};
pub use chunk::{KnowledgeChunkSpecV1, KnowledgeChunkV1};
pub use document::{
    KnowledgeContentReferenceV1, KnowledgeDocumentSourceV1, KnowledgeDocumentSpecV1,
    KnowledgeDocumentV1,
};
pub use document_catalog::{
    CreateKnowledgeChunk, CreateKnowledgeDocument, IKnowledgeChunkRepository,
    IKnowledgeDocumentRepository, KnowledgeChunkRecord, KnowledgeDocumentRecord,
};
pub use external_binding::{ExternalKnowledgeBindingSpecV1, ExternalKnowledgeBindingV1};
pub use index_revision::{KnowledgeIndexRevisionSpecV1, KnowledgeIndexRevisionV1};
pub use pipeline_release::{KnowledgePipelineReleaseSpecV1, KnowledgePipelineReleaseV1};
pub use retrieval_policy::{
    KnowledgeRetrievalPolicyRevisionSpecV1, KnowledgeRetrievalPolicyRevisionV1,
};
pub use types::{
    KnowledgeChunkStructureV1, KnowledgeIndexStrategyV1, EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1,
    KNOWLEDGE_BASE_REVISION_SCHEMA_V1, KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
    KNOWLEDGE_DOCUMENT_SCHEMA_V1, KNOWLEDGE_INDEX_REVISION_SCHEMA_V1,
    KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1, KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1,
};
pub use writes::{
    AppendKnowledgeBaseWrite, CreateKnowledgeBaseWrite, CreateKnowledgeChunkWrite,
    CreateKnowledgeDocumentWrite, CreateKnowledgePipelineWrite, KnowledgeBaseLifecycleChanged,
    KnowledgeBaseWriteReference, KnowledgeChunkLifecycleChanged, KnowledgeChunkWriteReference,
    KnowledgeDocumentLifecycleChanged, KnowledgeDocumentWriteReference,
    KnowledgePipelineLifecycleChanged, KnowledgePipelineWriteReference,
    PublishKnowledgePipelineWrite, KNOWLEDGE_BASE_LIFECYCLE_EVENT_SCHEMA,
    KNOWLEDGE_CHUNK_LIFECYCLE_EVENT_SCHEMA, KNOWLEDGE_DOCUMENT_LIFECYCLE_EVENT_SCHEMA,
    KNOWLEDGE_PIPELINE_LIFECYCLE_EVENT_SCHEMA,
};
