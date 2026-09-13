//! K0 Knowledge authority.
//!
//! `K0.1-C3` freezes canonical Knowledge and KnowledgePipeline contracts only.
//! Persistence, search indexes, provider clients, DAG engines, and workers
//! remain later gates.

mod domain;

pub use domain::{
    ExternalKnowledgeBindingSpecV1, ExternalKnowledgeBindingV1, KnowledgeBaseRevisionSpecV1,
    KnowledgeBaseRevisionV1, KnowledgeChunkSpecV1, KnowledgeChunkStructureV1, KnowledgeChunkV1,
    KnowledgeContentReferenceV1, KnowledgeDocumentSourceV1, KnowledgeDocumentSpecV1,
    KnowledgeDocumentV1, KnowledgeIndexRevisionSpecV1, KnowledgeIndexRevisionV1,
    KnowledgeIndexStrategyV1, KnowledgePipelineReleaseSpecV1, KnowledgePipelineReleaseV1,
    KnowledgeRetrievalPolicyRevisionSpecV1, KnowledgeRetrievalPolicyRevisionV1,
    EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1, KNOWLEDGE_BASE_REVISION_SCHEMA_V1,
    KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_DOCUMENT_SCHEMA_V1,
    KNOWLEDGE_INDEX_REVISION_SCHEMA_V1, KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1,
};
