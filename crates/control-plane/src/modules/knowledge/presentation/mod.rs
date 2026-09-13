mod controller;
mod dto;
mod knowledge_module;

pub const KNOWLEDGE_CONTROLLER_PREFIX: &str = "/organizations";
pub const KNOWLEDGE_BASE_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-bases";
pub const KNOWLEDGE_BASE_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-bases/{knowledge_base_id}";
pub const KNOWLEDGE_BASE_REVISION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-bases/{knowledge_base_id}/revisions";
pub const KNOWLEDGE_PIPELINE_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-pipelines";
pub const KNOWLEDGE_PIPELINE_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-pipelines/{pipeline_id}";
pub const KNOWLEDGE_PIPELINE_RELEASE_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-pipelines/{pipeline_id}/releases";
pub const KNOWLEDGE_DOCUMENT_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-documents";
pub const KNOWLEDGE_DOCUMENT_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-documents/{document_id}";
pub const KNOWLEDGE_DOCUMENT_CHUNK_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-documents/{document_id}/chunks";
pub const KNOWLEDGE_CHUNK_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/knowledge-chunks/{chunk_id}";

pub use controller::{knowledge_commands_controller, knowledge_queries_controller};
pub use dto::{
    AppendKnowledgeBaseRequest, CreateKnowledgeBaseRequest, CreateKnowledgeChunkRequest,
    CreateKnowledgeDocumentRequest, CreateKnowledgePipelineRequest, KnowledgeBaseMutationResponse,
    KnowledgeBaseResponse, KnowledgeChunkMutationResponse, KnowledgeChunkResponse,
    KnowledgeDocumentMutationResponse, KnowledgeDocumentResponse,
    KnowledgePipelineMutationResponse, KnowledgePipelineResponse, PublishKnowledgePipelineRequest,
};
pub use knowledge_module::KnowledgeModule;
