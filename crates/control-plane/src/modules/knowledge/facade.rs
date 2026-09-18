//! Deliberate public Knowledge persistence adapters.
//!
//! The concrete `infrastructure` module stays private. Selected repository
//! adapters are re-exported from the bounded-context root so composition and
//! tests never depend on an outer-layer module path.

pub use super::infrastructure::{
    InMemoryExternalKnowledgeBindingRepository, InMemoryKnowledgeBaseRepository,
    InMemoryKnowledgeChunkRepository, InMemoryKnowledgeDocumentRepository,
    InMemoryKnowledgeIndexRevisionRepository, InMemoryKnowledgePipelineRepository,
    InMemoryKnowledgeRetrievalPolicyRevisionRepository,
    PostgresExternalKnowledgeBindingRepository, PostgresKnowledgeBaseRepository,
    PostgresKnowledgeChunkRepository, PostgresKnowledgeDocumentRepository,
    PostgresKnowledgeIndexRevisionRepository, PostgresKnowledgePipelineRepository,
    PostgresKnowledgeRetrievalPolicyRevisionRepository,
};
