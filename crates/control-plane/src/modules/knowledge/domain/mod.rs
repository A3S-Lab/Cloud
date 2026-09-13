mod acl;
mod base_revision;
mod catalog;
mod chunk;
mod document;
mod external_binding;
mod index_revision;
mod pipeline_release;
mod retrieval_policy;
mod types;

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
