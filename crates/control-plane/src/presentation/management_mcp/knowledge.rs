use super::{arguments, tool_result};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::knowledge::{
    AppendKnowledgeBaseCommand, CreateExternalKnowledgeBindingCommand, CreateKnowledgeBaseCommand,
    CreateKnowledgeChunkCommand, CreateKnowledgeDocumentCommand,
    CreateKnowledgeIndexRevisionCommand, CreateKnowledgePipelineCommand,
    CreateKnowledgeRetrievalPolicyRevisionCommand, ExternalKnowledgeBindingMutationResponse,
    ExternalKnowledgeBindingResponse, GetExternalKnowledgeBinding, GetKnowledgeBase,
    GetKnowledgeChunk, GetKnowledgeDocument, GetKnowledgeIndexRevision, GetKnowledgePipeline,
    GetKnowledgeRetrievalPolicyRevision, KnowledgeBaseMutationResponse, KnowledgeBaseResponse,
    KnowledgeChunkMutationResponse, KnowledgeChunkResponse, KnowledgeDocumentMutationResponse,
    KnowledgeDocumentResponse, KnowledgeIndexRevisionMutationResponse,
    KnowledgeIndexRevisionResponse, KnowledgePipelineMutationResponse, KnowledgePipelineResponse,
    KnowledgeRetrievalPolicyRevisionMutationResponse, KnowledgeRetrievalPolicyRevisionResponse,
    ListExternalKnowledgeBindings, ListKnowledgeBases, ListKnowledgeChunks,
    ListKnowledgeDocuments, ListKnowledgeIndexRevisions, ListKnowledgePipelines,
    ListKnowledgeRetrievalPolicyRevisions, PublishKnowledgePipelineCommand,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId};
use crate::presentation::knowledge_access;
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeBaseArguments {
    project_id: Uuid,
    revision_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListKnowledgeBasesArguments {
    project_id: Uuid,
    #[serde(
        default = "arguments::default_list_limit",
        deserialize_with = "arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeBaseArguments {
    project_id: Uuid,
    knowledge_base_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendKnowledgeBaseArguments {
    project_id: Uuid,
    knowledge_base_id: Uuid,
    expected_revision_digest: String,
    revision_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgePipelineArguments {
    project_id: Uuid,
    release_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListKnowledgePipelinesArguments {
    project_id: Uuid,
    #[serde(
        default = "arguments::default_list_limit",
        deserialize_with = "arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgePipelineArguments {
    project_id: Uuid,
    pipeline_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishKnowledgePipelineArguments {
    project_id: Uuid,
    pipeline_id: Uuid,
    expected_release_digest: String,
    release_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

pub async fn create_base(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateKnowledgeBaseArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateKnowledgeBaseCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            revision_acl: arguments.revision_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            KnowledgeBaseMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_bases(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListKnowledgeBasesArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListKnowledgeBases {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            limit: Some(arguments.limit),
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(records) => tool_result::success(
            200,
            records
                .into_iter()
                .map(KnowledgeBaseResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_base(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: KnowledgeBaseArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetKnowledgeBase {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            knowledge_base_id: arguments.knowledge_base_id,
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(record) => tool_result::success(200, KnowledgeBaseResponse::from(record), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn append_base(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: AppendKnowledgeBaseArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(AppendKnowledgeBaseCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            knowledge_base_id: arguments.knowledge_base_id,
            expected_revision_digest: arguments.expected_revision_digest,
            revision_acl: arguments.revision_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            tool_result::success(200, KnowledgeBaseMutationResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_pipeline(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateKnowledgePipelineArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateKnowledgePipelineCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            release_acl: arguments.release_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            KnowledgePipelineMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_pipelines(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListKnowledgePipelinesArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListKnowledgePipelines {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            limit: Some(arguments.limit),
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(records) => tool_result::success(
            200,
            records
                .into_iter()
                .map(KnowledgePipelineResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_pipeline(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: KnowledgePipelineArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetKnowledgePipeline {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            pipeline_id: arguments.pipeline_id,
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(record) => {
            tool_result::success(200, KnowledgePipelineResponse::from(record), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn publish_pipeline(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: PublishKnowledgePipelineArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(PublishKnowledgePipelineCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            pipeline_id: arguments.pipeline_id,
            expected_release_digest: arguments.expected_release_digest,
            release_acl: arguments.release_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            KnowledgePipelineMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeDocumentArguments {
    project_id: Uuid,
    document_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeDocumentArguments {
    project_id: Uuid,
    document_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeChunkArguments {
    project_id: Uuid,
    document_id: Uuid,
    chunk_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeChunkArguments {
    project_id: Uuid,
    chunk_id: Uuid,
}

pub async fn create_document(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateKnowledgeDocumentArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateKnowledgeDocumentCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            document_acl: arguments.document_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            KnowledgeDocumentMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_document(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: KnowledgeDocumentArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetKnowledgeDocument {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            document_id: arguments.document_id,
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(record) => {
            tool_result::success(200, KnowledgeDocumentResponse::from(record), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_chunk(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateKnowledgeChunkArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateKnowledgeChunkCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            document_id: arguments.document_id,
            chunk_acl: arguments.chunk_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            KnowledgeChunkMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_chunk(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: KnowledgeChunkArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetKnowledgeChunk {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            chunk_id: arguments.chunk_id,
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(record) => tool_result::success(200, KnowledgeChunkResponse::from(record), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListKnowledgeDocumentsArguments {
    project_id: Uuid,
    knowledge_base_id: Uuid,
    #[serde(
        default = "arguments::default_list_limit",
        deserialize_with = "arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListKnowledgeChunksArguments {
    project_id: Uuid,
    document_id: Uuid,
    #[serde(
        default = "arguments::default_list_limit",
        deserialize_with = "arguments::deserialize_list_limit"
    )]
    limit: usize,
}

pub async fn list_documents(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListKnowledgeDocumentsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListKnowledgeDocuments {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            knowledge_base_id: arguments.knowledge_base_id,
            limit: Some(arguments.limit),
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(records) => tool_result::success(
            200,
            records
                .into_iter()
                .map(KnowledgeDocumentResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_chunks(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListKnowledgeChunksArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListKnowledgeChunks {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            document_id: arguments.document_id,
            limit: Some(arguments.limit),
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(records) => tool_result::success(
            200,
            records
                .into_iter()
                .map(KnowledgeChunkResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}


#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListKnowledgeIndexRevisionsArguments {
    project_id: Uuid,
    knowledge_base_revision_id: Uuid,
    #[serde(
        default = "arguments::default_list_limit",
        deserialize_with = "arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListKnowledgeRetrievalPolicyRevisionsArguments {
    project_id: Uuid,
    knowledge_base_revision_id: Uuid,
    #[serde(
        default = "arguments::default_list_limit",
        deserialize_with = "arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListExternalKnowledgeBindingsArguments {
    project_id: Uuid,
    knowledge_base_id: Uuid,
    #[serde(
        default = "arguments::default_list_limit",
        deserialize_with = "arguments::deserialize_list_limit"
    )]
    limit: usize,
}

pub async fn list_index_revisions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListKnowledgeIndexRevisionsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListKnowledgeIndexRevisions {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            knowledge_base_revision_id: arguments.knowledge_base_revision_id,
            limit: Some(arguments.limit),
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(records) => tool_result::success(
            200,
            records
                .into_iter()
                .map(KnowledgeIndexRevisionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_retrieval_policy_revisions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListKnowledgeRetrievalPolicyRevisionsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListKnowledgeRetrievalPolicyRevisions {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            knowledge_base_revision_id: arguments.knowledge_base_revision_id,
            limit: Some(arguments.limit),
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(records) => tool_result::success(
            200,
            records
                .into_iter()
                .map(KnowledgeRetrievalPolicyRevisionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_external_bindings(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListExternalKnowledgeBindingsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListExternalKnowledgeBindings {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            knowledge_base_id: arguments.knowledge_base_id,
            limit: Some(arguments.limit),
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(records) => tool_result::success(
            200,
            records
                .into_iter()
                .map(ExternalKnowledgeBindingResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeIndexRevisionArguments {
    project_id: Uuid,
    index_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeIndexRevisionArguments {
    project_id: Uuid,
    index_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateKnowledgeRetrievalPolicyRevisionArguments {
    project_id: Uuid,
    policy_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeRetrievalPolicyRevisionArguments {
    project_id: Uuid,
    policy_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateExternalKnowledgeBindingArguments {
    project_id: Uuid,
    binding_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalKnowledgeBindingArguments {
    project_id: Uuid,
    binding_id: Uuid,
}

pub async fn create_index_revision(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateKnowledgeIndexRevisionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateKnowledgeIndexRevisionCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            index_acl: arguments.index_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            KnowledgeIndexRevisionMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_index_revision(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: KnowledgeIndexRevisionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetKnowledgeIndexRevision {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            index_revision_id: arguments.index_revision_id,
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(record) => tool_result::success(
            200,
            KnowledgeIndexRevisionResponse::from(record),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_retrieval_policy_revision(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateKnowledgeRetrievalPolicyRevisionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateKnowledgeRetrievalPolicyRevisionCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            policy_acl: arguments.policy_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            KnowledgeRetrievalPolicyRevisionMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_retrieval_policy_revision(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: KnowledgeRetrievalPolicyRevisionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetKnowledgeRetrievalPolicyRevision {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            policy_revision_id: arguments.policy_revision_id,
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(record) => tool_result::success(
            200,
            KnowledgeRetrievalPolicyRevisionResponse::from(record),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_external_binding(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateExternalKnowledgeBindingArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateExternalKnowledgeBindingCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            binding_acl: arguments.binding_acl,
            actor_principal_id,
            access: knowledge_access(&resource_access),
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ExternalKnowledgeBindingMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_external_binding(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ExternalKnowledgeBindingArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetExternalKnowledgeBinding {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            binding_id: arguments.binding_id,
            access: knowledge_access(&resource_access),
        })
        .await?
    {
        Ok(record) => tool_result::success(
            200,
            ExternalKnowledgeBindingResponse::from(record),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
