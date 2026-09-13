//! K0 Knowledge authority.
//!
//! `K0.1-C3` freezes canonical Knowledge and KnowledgePipeline contracts.
//! `K0.1-C4a` adds the durable KnowledgeBase/PipelineRelease catalogs without
//! search indexes, provider clients, DAG engines, workers, or public surfaces.
//! `K0.1-C4b1` adds application owner catalog services over those repositories.
//! `K0.1-C4b2a` adds authorized idempotent create/append/publish writes with
//! shared audit and Outbox side effects.
//! `K0.1-C4b2b1` exposes that mutation boundary over REST/OpenAPI with CQRS
//! handlers and control-plane wiring. `K0.1-C4b2b2` adds the maintained client,
//! CLI, and Management MCP authority over the same catalog lifecycle handlers.
//! `K0.1-C5` persists immutable KnowledgeDocument and KnowledgeChunk catalogs
//! through migration `202` without search indexes, ingestion workers,
//! authorization surfaces, or public REST/MCP interfaces.
//! `K0.1-C6` adds application owner document/chunk catalog services over
//! those repositories so presentation cannot reach persistence adapters
//! directly. No authorization, idempotency, audit, Outbox, REST/OpenAPI,
//! client, CLI, or Management MCP surface.
//! `K0.1-C7` adds authorized idempotent KnowledgeDocument/Chunk create writes
//! with audit and Outbox side effects over the C5 repositories. No REST/MCP.
//! `K0.1-C8` exposes that document/chunk mutation boundary over REST/OpenAPI
//! with CQRS handlers and control-plane wiring. No client/CLI/MCP or live
//! MinIO/scanner/SEV claims.

mod application;
mod domain;
mod infrastructure;
mod presentation;

pub use application::{
    AppendKnowledgeBaseCommand, AppendKnowledgeBaseHandler, CreateKnowledgeBaseCommand,
    CreateKnowledgeBaseHandler, CreateKnowledgeChunkCommand, CreateKnowledgeChunkHandler,
    CreateKnowledgeDocumentCommand, CreateKnowledgeDocumentHandler, CreateKnowledgePipelineCommand,
    CreateKnowledgePipelineHandler, GetKnowledgeBase, GetKnowledgeBaseHandler, GetKnowledgeChunk,
    GetKnowledgeChunkHandler, GetKnowledgeDocument, GetKnowledgeDocumentHandler,
    GetKnowledgePipeline, GetKnowledgePipelineHandler, KnowledgeAccess,
    KnowledgeBaseCatalogService, KnowledgeCatalogLifecycleService, KnowledgeChunkCatalogService,
    KnowledgeDocumentCatalogService, KnowledgeDocumentLifecycleService, KnowledgeMutationResult,
    KnowledgePipelineCatalogService, ListKnowledgeBases, ListKnowledgeBasesHandler,
    ListKnowledgePipelines, ListKnowledgePipelinesHandler, PublishKnowledgePipelineCommand,
    PublishKnowledgePipelineHandler, DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT,
    DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT, MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
};
pub use domain::{
    AppendKnowledgeBaseRevision, AppendKnowledgeBaseWrite, CreateKnowledgeBase,
    CreateKnowledgeBaseWrite, CreateKnowledgeChunk, CreateKnowledgeChunkWrite,
    CreateKnowledgeDocument, CreateKnowledgeDocumentWrite, CreateKnowledgePipeline,
    CreateKnowledgePipelineWrite, ExternalKnowledgeBindingSpecV1, ExternalKnowledgeBindingV1,
    IKnowledgeBaseRepository, IKnowledgeChunkRepository, IKnowledgeDocumentRepository,
    IKnowledgePipelineRepository, KnowledgeBaseLifecycleChanged, KnowledgeBaseRecord,
    KnowledgeBaseRevisionSpecV1, KnowledgeBaseRevisionV1, KnowledgeBaseWriteReference,
    KnowledgeChunkRecord, KnowledgeChunkSpecV1, KnowledgeChunkStructureV1, KnowledgeChunkV1,
    KnowledgeContentReferenceV1, KnowledgeDocumentLifecycleChanged, KnowledgeDocumentRecord,
    KnowledgeDocumentSourceV1, KnowledgeDocumentSpecV1, KnowledgeDocumentV1,
    KnowledgeDocumentWriteReference, KnowledgeIndexRevisionSpecV1, KnowledgeIndexRevisionV1,
    KnowledgeIndexStrategyV1, KnowledgePipelineLifecycleChanged, KnowledgePipelineRecord,
    KnowledgePipelineReleaseSpecV1, KnowledgePipelineReleaseV1, KnowledgePipelineWriteReference,
    KnowledgeRetrievalPolicyRevisionSpecV1, KnowledgeRetrievalPolicyRevisionV1,
    PublishKnowledgePipelineRelease, PublishKnowledgePipelineWrite,
    EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1, KNOWLEDGE_BASE_LIFECYCLE_EVENT_SCHEMA,
    KNOWLEDGE_BASE_REVISION_SCHEMA_V1, KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
    KNOWLEDGE_DOCUMENT_SCHEMA_V1, KNOWLEDGE_INDEX_REVISION_SCHEMA_V1,
    KNOWLEDGE_PIPELINE_LIFECYCLE_EVENT_SCHEMA, KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1,
};
pub use infrastructure::{
    InMemoryKnowledgeBaseRepository, InMemoryKnowledgeChunkRepository,
    InMemoryKnowledgeDocumentRepository, InMemoryKnowledgePipelineRepository,
    PostgresKnowledgeBaseRepository, PostgresKnowledgeChunkRepository,
    PostgresKnowledgeDocumentRepository, PostgresKnowledgePipelineRepository,
};
pub(crate) use presentation::{
    KnowledgeBaseMutationResponse, KnowledgeBaseResponse, KnowledgeModule,
    KnowledgePipelineMutationResponse, KnowledgePipelineResponse, KNOWLEDGE_BASE_COLLECTION_ROUTE,
    KNOWLEDGE_BASE_ITEM_ROUTE, KNOWLEDGE_BASE_REVISION_ROUTE, KNOWLEDGE_CHUNK_ITEM_ROUTE,
    KNOWLEDGE_CONTROLLER_PREFIX, KNOWLEDGE_DOCUMENT_CHUNK_COLLECTION_ROUTE,
    KNOWLEDGE_DOCUMENT_COLLECTION_ROUTE, KNOWLEDGE_DOCUMENT_ITEM_ROUTE,
    KNOWLEDGE_PIPELINE_COLLECTION_ROUTE, KNOWLEDGE_PIPELINE_ITEM_ROUTE,
    KNOWLEDGE_PIPELINE_RELEASE_ROUTE,
};
