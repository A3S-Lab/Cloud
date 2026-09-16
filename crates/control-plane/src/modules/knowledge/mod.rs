//! K0 Knowledge authority.
//!
//! `K0.2-C1` freezes file-upload and inline-text datasource entrance
//! contracts whose digests feed `KnowledgePipelineRelease`
//! `datasource_entrance_digests`. Drive/crawl/marketplace kinds remain
//! deferred.
//! `K0.2-C2` freezes built-in plain-text processor output contracts
//! whose digests feed `output_contract_digest`. Tool/OCR/multimodal
//! processors remain deferred.
//! `K0.2-C3` freezes file/text+builtin-text ingestion provenance
//! whose digests feed document `provenance_digest`. Drive/crawl/
//! Tool/OCR provenance remains deferred. No persistence, worker, or
//! availability.
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
//! `K0.1-C9` adds maintained client, CLI, and Management MCP over the same
//! document/chunk handlers without bumping OpenAPI.
//! `K0.1-C10` adds authorized bounded document/chunk list through REST/OpenAPI
//! `1.94.0`, client, CLI, and Management MCP read tools.
//! `K0.1-C11` persists immutable KnowledgeIndexRevision,
//! KnowledgeRetrievalPolicyRevision, and ExternalKnowledgeBinding catalogs
//! through migration `203` with local and PostgreSQL adapters and no public
//! surface.
//! `K0.1-C12` adds application owner index/policy/binding catalog services over
//! those repositories so presentation cannot reach persistence adapters
//! directly. No authorization, idempotency, audit, Outbox, REST/OpenAPI,
//! client, CLI, or Management MCP surface.
//! `K0.1-C13` adds authorized idempotent index/policy/binding create writes with
//! audit and Outbox side effects over the C11 repositories. No REST/MCP.
//! `K0.1-C14` exposes that index/policy/binding mutation boundary over
//! REST/OpenAPI `1.95.0` with CQRS handlers and control-plane wiring. No
//! client/CLI/MCP or live MinIO/scanner/SEV claims.
//! `K0.1-C15` adds maintained client, CLI, and Management MCP over the same
//! index/policy/binding handlers without bumping OpenAPI.
//! `K0.1-C16` adds authorized bounded list for index/policy/binding through
//! REST/OpenAPI `1.96.0`, client, CLI, and three Management MCP read tools.

mod application;
mod domain;
mod infrastructure;
mod presentation;

pub use application::{
    AppendKnowledgeBaseCommand, AppendKnowledgeBaseHandler, CreateExternalKnowledgeBindingCommand,
    CreateExternalKnowledgeBindingHandler, CreateKnowledgeBaseCommand, CreateKnowledgeBaseHandler,
    CreateKnowledgeChunkCommand, CreateKnowledgeChunkHandler, CreateKnowledgeDocumentCommand,
    CreateKnowledgeDocumentHandler, CreateKnowledgeIndexRevisionCommand,
    CreateKnowledgeIndexRevisionHandler, CreateKnowledgePipelineCommand,
    CreateKnowledgePipelineHandler, CreateKnowledgeRetrievalPolicyRevisionCommand,
    CreateKnowledgeRetrievalPolicyRevisionHandler, ExternalKnowledgeBindingCatalogService,
    GetExternalKnowledgeBinding, GetExternalKnowledgeBindingHandler, GetKnowledgeBase,
    GetKnowledgeBaseHandler, GetKnowledgeChunk, GetKnowledgeChunkHandler, GetKnowledgeDocument,
    GetKnowledgeDocumentHandler, GetKnowledgeIndexRevision, GetKnowledgeIndexRevisionHandler,
    GetKnowledgePipeline, GetKnowledgePipelineHandler, GetKnowledgeRetrievalPolicyRevision,
    GetKnowledgeRetrievalPolicyRevisionHandler, KnowledgeAccess, KnowledgeBaseCatalogService,
    KnowledgeCatalogLifecycleService, KnowledgeChunkCatalogService, KnowledgeDocumentCatalogService,
    KnowledgeDocumentLifecycleService, KnowledgeIndexLifecycleService,
    KnowledgeIndexRevisionCatalogService, KnowledgeMutationResult,
    KnowledgePipelineCatalogService, KnowledgeRetrievalPolicyRevisionCatalogService,
    ListExternalKnowledgeBindings, ListExternalKnowledgeBindingsHandler, ListKnowledgeBases,
    ListKnowledgeBasesHandler, ListKnowledgeChunks, ListKnowledgeChunksHandler,
    ListKnowledgeDocuments, ListKnowledgeDocumentsHandler, ListKnowledgeIndexRevisions,
    ListKnowledgeIndexRevisionsHandler, ListKnowledgePipelines, ListKnowledgePipelinesHandler,
    ListKnowledgeRetrievalPolicyRevisions, ListKnowledgeRetrievalPolicyRevisionsHandler,
    PublishKnowledgePipelineCommand,
    PublishKnowledgePipelineHandler, DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT,
    DEFAULT_EXTERNAL_KNOWLEDGE_BINDING_LIST_LIMIT, DEFAULT_KNOWLEDGE_CHUNK_LIST_LIMIT,
    DEFAULT_KNOWLEDGE_DOCUMENT_LIST_LIMIT, DEFAULT_KNOWLEDGE_INDEX_REVISION_LIST_LIMIT,
    DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT,
    DEFAULT_KNOWLEDGE_RETRIEVAL_POLICY_REVISION_LIST_LIMIT, MAXIMUM_EXTERNAL_KNOWLEDGE_BINDING_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT, MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_DOCUMENT_LIST_LIMIT, MAXIMUM_KNOWLEDGE_INDEX_REVISION_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_RETRIEVAL_POLICY_REVISION_LIST_LIMIT,
};
pub use domain::{
    AppendKnowledgeBaseRevision, AppendKnowledgeBaseWrite, CreateKnowledgeBase,
    CreateKnowledgeBaseWrite, CreateKnowledgeChunk, CreateKnowledgeChunkWrite,
    CreateKnowledgeDocument, CreateKnowledgeDocumentWrite, CreateKnowledgePipeline,
    CreateKnowledgePipelineWrite, ExternalKnowledgeBindingSpecV1, ExternalKnowledgeBindingV1,
    CreateExternalKnowledgeBinding, CreateExternalKnowledgeBindingWrite,
    CreateKnowledgeIndexRevision, CreateKnowledgeIndexRevisionWrite,
    CreateKnowledgeRetrievalPolicyRevision, CreateKnowledgeRetrievalPolicyRevisionWrite,
    ExternalKnowledgeBindingRecord,
    IExternalKnowledgeBindingRepository, IKnowledgeBaseRepository, IKnowledgeChunkRepository,
    IKnowledgeDocumentRepository, IKnowledgeIndexRevisionRepository,
    IKnowledgePipelineRepository, IKnowledgeRetrievalPolicyRevisionRepository,
    KnowledgeBaseLifecycleChanged, KnowledgeBaseRecord, KnowledgeIndexRevisionRecord,
    KnowledgeRetrievalPolicyRevisionRecord,
    KnowledgeBaseRevisionSpecV1, KnowledgeBaseRevisionV1, KnowledgeBaseWriteReference,
    KnowledgeChunkRecord, KnowledgeChunkSpecV1, KnowledgeChunkStructureV1, KnowledgeChunkV1,
    KnowledgeContentReferenceV1, KnowledgeDatasourceEntranceKindV1,
    KnowledgeDatasourceEntranceSpecV1, KnowledgeDatasourceEntranceV1,
    KnowledgeIngestionProvenanceKindV1, KnowledgeIngestionProvenanceSpecV1,
    KnowledgeIngestionProvenanceV1, KnowledgeProcessorOutputContractKindV1,
    KnowledgeProcessorOutputContractSpecV1, KnowledgeProcessorOutputContractV1,
    KnowledgeDocumentLifecycleChanged, KnowledgeDocumentRecord,
    KnowledgeDocumentSourceV1, KnowledgeDocumentSpecV1, KnowledgeDocumentV1,
    KnowledgeDocumentWriteReference, KnowledgeIndexRevisionSpecV1, KnowledgeIndexRevisionV1,
    KnowledgeIndexStrategyV1, KnowledgePipelineLifecycleChanged, KnowledgePipelineRecord,
    KnowledgePipelineReleaseSpecV1, KnowledgePipelineReleaseV1, KnowledgePipelineWriteReference,
    KnowledgeRetrievalPolicyRevisionSpecV1, KnowledgeRetrievalPolicyRevisionV1,
    PublishKnowledgePipelineRelease, PublishKnowledgePipelineWrite,
    EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1, KNOWLEDGE_BASE_LIFECYCLE_EVENT_SCHEMA,
    KNOWLEDGE_BASE_REVISION_SCHEMA_V1, KNOWLEDGE_CHUNK_SCHEMA_V1, KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
    KNOWLEDGE_DATASOURCE_ENTRANCE_SCHEMA_V1, KNOWLEDGE_DOCUMENT_SCHEMA_V1,
    KNOWLEDGE_INGESTION_PROVENANCE_SCHEMA_V1,
    KNOWLEDGE_PROCESSOR_OUTPUT_CONTRACT_SCHEMA_V1,
    KNOWLEDGE_INDEX_REVISION_SCHEMA_V1,
    KNOWLEDGE_PIPELINE_LIFECYCLE_EVENT_SCHEMA, KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1,
};
pub use infrastructure::{
    InMemoryExternalKnowledgeBindingRepository, InMemoryKnowledgeBaseRepository,
    InMemoryKnowledgeChunkRepository, InMemoryKnowledgeDocumentRepository,
    InMemoryKnowledgeIndexRevisionRepository, InMemoryKnowledgePipelineRepository,
    InMemoryKnowledgeRetrievalPolicyRevisionRepository,
    PostgresExternalKnowledgeBindingRepository, PostgresKnowledgeBaseRepository,
    PostgresKnowledgeChunkRepository, PostgresKnowledgeDocumentRepository,
    PostgresKnowledgeIndexRevisionRepository, PostgresKnowledgePipelineRepository,
    PostgresKnowledgeRetrievalPolicyRevisionRepository,
};
pub(crate) use presentation::{
    ExternalKnowledgeBindingMutationResponse, ExternalKnowledgeBindingResponse,
    KnowledgeBaseMutationResponse, KnowledgeBaseResponse, KnowledgeChunkMutationResponse,
    KnowledgeChunkResponse, KnowledgeDocumentMutationResponse, KnowledgeDocumentResponse,
    KnowledgeIndexRevisionMutationResponse, KnowledgeIndexRevisionResponse, KnowledgeModule,
    KnowledgePipelineMutationResponse, KnowledgePipelineResponse,
    KnowledgeRetrievalPolicyRevisionMutationResponse, KnowledgeRetrievalPolicyRevisionResponse,
    EXTERNAL_KNOWLEDGE_BINDING_COLLECTION_ROUTE, EXTERNAL_KNOWLEDGE_BINDING_ITEM_ROUTE,
    KNOWLEDGE_BASE_COLLECTION_ROUTE, KNOWLEDGE_BASE_ITEM_ROUTE, KNOWLEDGE_BASE_REVISION_ROUTE,
    KNOWLEDGE_CHUNK_ITEM_ROUTE, KNOWLEDGE_CONTROLLER_PREFIX,
    KNOWLEDGE_DOCUMENT_CHUNK_COLLECTION_ROUTE, KNOWLEDGE_DOCUMENT_COLLECTION_ROUTE,
    KNOWLEDGE_DOCUMENT_ITEM_ROUTE, KNOWLEDGE_INDEX_REVISION_COLLECTION_ROUTE,
    KNOWLEDGE_INDEX_REVISION_ITEM_ROUTE, KNOWLEDGE_PIPELINE_COLLECTION_ROUTE,
    KNOWLEDGE_PIPELINE_ITEM_ROUTE, KNOWLEDGE_PIPELINE_RELEASE_ROUTE,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_COLLECTION_ROUTE,
    KNOWLEDGE_RETRIEVAL_POLICY_REVISION_ITEM_ROUTE,
};
