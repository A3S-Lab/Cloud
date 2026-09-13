//! K0 Knowledge authority.
//!
//! `K0.1-C3` freezes canonical Knowledge and KnowledgePipeline contracts.
//! `K0.1-C4a` adds the durable KnowledgeBase/PipelineRelease catalogs without
//! search indexes, provider clients, DAG engines, workers, or public surfaces.
//! `K0.1-C4b1` adds application owner catalog services over those repositories.
//! `K0.1-C4b2a` adds authorized idempotent create/append/publish writes with
//! shared audit and Outbox side effects.
//! `K0.1-C4b2b1` exposes that mutation boundary over REST/OpenAPI with CQRS
//! handlers and control-plane wiring. Maintained client, CLI, and Management MCP
//! remain deferred to `K0.1-C4b2b2`.

mod application;
mod domain;
mod infrastructure;
mod presentation;

pub use application::{
    AppendKnowledgeBaseCommand, AppendKnowledgeBaseHandler, CreateKnowledgeBaseCommand,
    CreateKnowledgeBaseHandler, CreateKnowledgePipelineCommand, CreateKnowledgePipelineHandler,
    GetKnowledgeBase, GetKnowledgeBaseHandler, GetKnowledgePipeline, GetKnowledgePipelineHandler,
    KnowledgeAccess, KnowledgeBaseCatalogService, KnowledgeCatalogLifecycleService,
    KnowledgeMutationResult, KnowledgePipelineCatalogService, ListKnowledgeBases,
    ListKnowledgeBasesHandler, ListKnowledgePipelines, ListKnowledgePipelinesHandler,
    PublishKnowledgePipelineCommand, PublishKnowledgePipelineHandler,
    DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT, DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT, MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
};
pub use domain::{
    AppendKnowledgeBaseRevision, AppendKnowledgeBaseWrite, CreateKnowledgeBase,
    CreateKnowledgeBaseWrite, CreateKnowledgePipeline, CreateKnowledgePipelineWrite,
    ExternalKnowledgeBindingSpecV1, ExternalKnowledgeBindingV1, IKnowledgeBaseRepository,
    IKnowledgePipelineRepository, KnowledgeBaseLifecycleChanged, KnowledgeBaseRecord,
    KnowledgeBaseRevisionSpecV1, KnowledgeBaseRevisionV1, KnowledgeBaseWriteReference,
    KnowledgeChunkSpecV1, KnowledgeChunkStructureV1, KnowledgeChunkV1, KnowledgeContentReferenceV1,
    KnowledgeDocumentSourceV1, KnowledgeDocumentSpecV1, KnowledgeDocumentV1,
    KnowledgeIndexRevisionSpecV1, KnowledgeIndexRevisionV1, KnowledgeIndexStrategyV1,
    KnowledgePipelineLifecycleChanged, KnowledgePipelineRecord, KnowledgePipelineReleaseSpecV1,
    KnowledgePipelineReleaseV1, KnowledgePipelineWriteReference,
    KnowledgeRetrievalPolicyRevisionSpecV1, KnowledgeRetrievalPolicyRevisionV1,
    PublishKnowledgePipelineRelease, PublishKnowledgePipelineWrite,
    EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1, KNOWLEDGE_BASE_LIFECYCLE_EVENT_SCHEMA,
    KNOWLEDGE_BASE_REVISION_SCHEMA_V1, KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
    KNOWLEDGE_DOCUMENT_SCHEMA_V1, KNOWLEDGE_INDEX_REVISION_SCHEMA_V1,
    KNOWLEDGE_PIPELINE_LIFECYCLE_EVENT_SCHEMA, KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1,
};
pub use infrastructure::{
    InMemoryKnowledgeBaseRepository, InMemoryKnowledgePipelineRepository,
    PostgresKnowledgeBaseRepository, PostgresKnowledgePipelineRepository,
};
pub(crate) use presentation::{
    KnowledgeModule, KNOWLEDGE_BASE_COLLECTION_ROUTE, KNOWLEDGE_BASE_ITEM_ROUTE,
    KNOWLEDGE_BASE_REVISION_ROUTE, KNOWLEDGE_CONTROLLER_PREFIX,
    KNOWLEDGE_PIPELINE_COLLECTION_ROUTE, KNOWLEDGE_PIPELINE_ITEM_ROUTE,
    KNOWLEDGE_PIPELINE_RELEASE_ROUTE,
};
