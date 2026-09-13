mod knowledge_document_in_memory;
mod knowledge_index_in_memory;
mod knowledge_index_postgres;
mod knowledge_document_postgres;
mod knowledge_in_memory;
mod knowledge_postgres;

pub use knowledge_document_in_memory::{
    InMemoryKnowledgeChunkRepository, InMemoryKnowledgeDocumentRepository,
};
pub use knowledge_document_postgres::{
    PostgresKnowledgeChunkRepository, PostgresKnowledgeDocumentRepository,
};
pub use knowledge_in_memory::{
    InMemoryKnowledgeBaseRepository, InMemoryKnowledgePipelineRepository,
};
pub use knowledge_postgres::{
    PostgresKnowledgeBaseRepository, PostgresKnowledgePipelineRepository,
};

pub use knowledge_index_in_memory::{
    InMemoryExternalKnowledgeBindingRepository, InMemoryKnowledgeIndexRevisionRepository,
    InMemoryKnowledgeRetrievalPolicyRevisionRepository,
};
pub use knowledge_index_postgres::{
    PostgresExternalKnowledgeBindingRepository, PostgresKnowledgeIndexRevisionRepository,
    PostgresKnowledgeRetrievalPolicyRevisionRepository,
};
