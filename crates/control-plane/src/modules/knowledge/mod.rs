//! K0 Knowledge authority.
//!
//! `K0.1-C3` freezes canonical Knowledge and KnowledgePipeline contracts.
//! `K0.1-C4a` adds the durable KnowledgeBase/PipelineRelease catalogs without
//! search indexes, provider clients, DAG engines, workers, or public surfaces.
//! `K0.1-C4b1` adds application owner catalog services over those repositories
//! without authorization, idempotency, audit, Outbox, or public HTTP/MCP.

mod application;
mod domain;
mod infrastructure;

pub use application::{KnowledgeBaseCatalogService, KnowledgePipelineCatalogService};
pub use domain::{
    AppendKnowledgeBaseRevision, CreateKnowledgeBase, CreateKnowledgePipeline,
    ExternalKnowledgeBindingSpecV1, ExternalKnowledgeBindingV1, IKnowledgeBaseRepository,
    IKnowledgePipelineRepository, KnowledgeBaseRecord, KnowledgeBaseRevisionSpecV1,
    KnowledgeBaseRevisionV1, KnowledgeChunkSpecV1, KnowledgeChunkStructureV1, KnowledgeChunkV1,
    KnowledgeContentReferenceV1, KnowledgeDocumentSourceV1, KnowledgeDocumentSpecV1,
    KnowledgeDocumentV1, KnowledgeIndexRevisionSpecV1, KnowledgeIndexRevisionV1,
    KnowledgeIndexStrategyV1, KnowledgePipelineRecord, KnowledgePipelineReleaseSpecV1,
    KnowledgePipelineReleaseV1, KnowledgeRetrievalPolicyRevisionSpecV1,
    KnowledgeRetrievalPolicyRevisionV1, PublishKnowledgePipelineRelease,
    EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1, KNOWLEDGE_BASE_REVISION_SCHEMA_V1,
    KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES, KNOWLEDGE_DOCUMENT_SCHEMA_V1,
    KNOWLEDGE_INDEX_REVISION_SCHEMA_V1, KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1,
};
pub use infrastructure::{
    InMemoryKnowledgeBaseRepository, InMemoryKnowledgePipelineRepository,
    PostgresKnowledgeBaseRepository, PostgresKnowledgePipelineRepository,
};
