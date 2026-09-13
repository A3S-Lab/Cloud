mod knowledge_in_memory;
mod knowledge_postgres;

pub use knowledge_in_memory::{
    InMemoryKnowledgeBaseRepository, InMemoryKnowledgePipelineRepository,
};
pub use knowledge_postgres::{
    PostgresKnowledgeBaseRepository, PostgresKnowledgePipelineRepository,
};
