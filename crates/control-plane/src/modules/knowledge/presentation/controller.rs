use super::{
    AppendKnowledgeBaseRequest, CreateExternalKnowledgeBindingRequest, CreateKnowledgeBaseRequest,
    CreateKnowledgeChunkRequest, CreateKnowledgeDocumentRequest,
    CreateKnowledgeIndexRevisionRequest, CreateKnowledgePipelineRequest,
    CreateKnowledgeRetrievalPolicyRevisionRequest, ExternalKnowledgeBindingMutationResponse,
    ExternalKnowledgeBindingResponse, KnowledgeBaseMutationResponse, KnowledgeBaseResponse,
    KnowledgeChunkMutationResponse, KnowledgeChunkResponse, KnowledgeDocumentMutationResponse,
    KnowledgeDocumentResponse, KnowledgeIndexRevisionMutationResponse,
    KnowledgeIndexRevisionResponse, KnowledgePipelineMutationResponse, KnowledgePipelineResponse,
    KnowledgeRetrievalPolicyRevisionMutationResponse, KnowledgeRetrievalPolicyRevisionResponse,
    PublishKnowledgePipelineRequest,
};
use crate::modules::knowledge::application::{
    AppendKnowledgeBaseCommand, CreateExternalKnowledgeBindingCommand, CreateKnowledgeBaseCommand,
    CreateKnowledgeChunkCommand, CreateKnowledgeDocumentCommand,
    CreateKnowledgeIndexRevisionCommand, CreateKnowledgePipelineCommand,
    CreateKnowledgeRetrievalPolicyRevisionCommand, DEFAULT_EXTERNAL_KNOWLEDGE_BINDING_LIST_LIMIT,
    DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT, DEFAULT_KNOWLEDGE_CHUNK_LIST_LIMIT,
    DEFAULT_KNOWLEDGE_DOCUMENT_LIST_LIMIT, DEFAULT_KNOWLEDGE_INDEX_REVISION_LIST_LIMIT,
    DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT, DEFAULT_KNOWLEDGE_RETRIEVAL_POLICY_REVISION_LIST_LIMIT,
    GetExternalKnowledgeBinding, GetKnowledgeBase, GetKnowledgeChunk, GetKnowledgeDocument,
    GetKnowledgeIndexRevision, GetKnowledgePipeline, GetKnowledgeRetrievalPolicyRevision,
    ListExternalKnowledgeBindings, ListKnowledgeBases, ListKnowledgeChunks, ListKnowledgeDocuments,
    ListKnowledgeIndexRevisions, ListKnowledgePipelines, ListKnowledgeRetrievalPolicyRevisions,
    MAXIMUM_EXTERNAL_KNOWLEDGE_BINDING_LIST_LIMIT, MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT, MAXIMUM_KNOWLEDGE_DOCUMENT_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_INDEX_REVISION_LIST_LIMIT, MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_RETRIEVAL_POLICY_REVISION_LIST_LIMIT, PublishKnowledgePipelineCommand,
};
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::presentation::{
    actor_principal_id, application_error_response, knowledge_access,
    organization_tenant_cloud_read_controller, organization_tenant_knowledge_write_controller,
    request_id, request_identity, resource_access_evaluator,
};
use a3s_boot::{
    AUTH_SCOPES_METADATA, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    QueryBus, Result, controller, get, post,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn knowledge_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let controller = Arc::new(KnowledgeCommandsController { bus }).controller()?;
    organization_tenant_knowledge_write_controller(controller)
}

pub fn knowledge_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let controller = Arc::new(KnowledgeQueriesController { bus }).controller()?;
    organization_tenant_cloud_read_controller(controller)
}

#[derive(Debug, Clone)]
struct KnowledgeCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct KnowledgeQueriesController {
    bus: Arc<QueryBus>,
}
#[controller("/organizations")]
impl KnowledgeCommandsController {
    #[post("/{organization_id}/projects/{project_id}/knowledge-bases", raw)]
    async fn create_knowledge_base(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateKnowledgeBaseRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateKnowledgeBaseCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                revision_acl: body.revision_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &KnowledgeBaseMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/knowledge-bases/{knowledge_base_id}/revisions",
        raw
    )]
    async fn append_knowledge_base(&self, request: BootRequest) -> Result<BootResponse> {
        let body: AppendKnowledgeBaseRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(AppendKnowledgeBaseCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                knowledge_base_id: request.param_as::<Uuid>("knowledge_base_id")?,
                expected_revision_digest: body.expected_revision_digest,
                revision_acl: body.revision_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json(&KnowledgeBaseMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/projects/{project_id}/knowledge-pipelines", raw)]
    async fn create_knowledge_pipeline(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateKnowledgePipelineRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateKnowledgePipelineCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                release_acl: body.release_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &KnowledgePipelineMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/knowledge-pipelines/{pipeline_id}/releases",
        raw
    )]
    async fn publish_knowledge_pipeline(&self, request: BootRequest) -> Result<BootResponse> {
        let body: PublishKnowledgePipelineRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(PublishKnowledgePipelineCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                pipeline_id: request.param_as::<Uuid>("pipeline_id")?,
                expected_release_digest: body.expected_release_digest,
                release_acl: body.release_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json(&KnowledgePipelineMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/projects/{project_id}/knowledge-documents", raw)]
    async fn create_knowledge_document(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateKnowledgeDocumentRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateKnowledgeDocumentCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                document_acl: body.document_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &KnowledgeDocumentMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/knowledge-documents/{document_id}/chunks",
        raw
    )]
    async fn create_knowledge_chunk(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateKnowledgeChunkRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateKnowledgeChunkCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                document_id: request.param_as::<Uuid>("document_id")?,
                chunk_acl: body.chunk_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &KnowledgeChunkMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/knowledge-index-revisions",
        raw
    )]
    async fn create_knowledge_index_revision(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateKnowledgeIndexRevisionRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateKnowledgeIndexRevisionCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                index_acl: body.index_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &KnowledgeIndexRevisionMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/knowledge-retrieval-policy-revisions",
        raw
    )]
    async fn create_knowledge_retrieval_policy_revision(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let body: CreateKnowledgeRetrievalPolicyRevisionRequest =
            request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateKnowledgeRetrievalPolicyRevisionCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                policy_acl: body.policy_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &KnowledgeRetrievalPolicyRevisionMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/external-knowledge-bindings",
        raw
    )]
    async fn create_external_knowledge_binding(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let body: CreateExternalKnowledgeBindingRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateExternalKnowledgeBindingCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                binding_acl: body.binding_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &ExternalKnowledgeBindingMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
impl KnowledgeQueriesController {
    #[get("/{organization_id}/projects/{project_id}/knowledge-bases", raw)]
    async fn list_knowledge_bases(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListKnowledgeBases {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                limit: Some(base_list_limit(&request)?),
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(records) => BootResponse::json(
                &records
                    .into_iter()
                    .map(KnowledgeBaseResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-bases/{knowledge_base_id}",
        raw
    )]
    async fn get_knowledge_base(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetKnowledgeBase {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                knowledge_base_id: request.param_as::<Uuid>("knowledge_base_id")?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(record) => BootResponse::json(&KnowledgeBaseResponse::from(record)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/projects/{project_id}/knowledge-pipelines", raw)]
    async fn list_knowledge_pipelines(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListKnowledgePipelines {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                limit: Some(pipeline_list_limit(&request)?),
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(records) => BootResponse::json(
                &records
                    .into_iter()
                    .map(KnowledgePipelineResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-pipelines/{pipeline_id}",
        raw
    )]
    async fn get_knowledge_pipeline(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetKnowledgePipeline {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                pipeline_id: request.param_as::<Uuid>("pipeline_id")?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(record) => BootResponse::json(&KnowledgePipelineResponse::from(record)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/projects/{project_id}/knowledge-documents", raw)]
    async fn list_knowledge_documents(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let knowledge_base_id = request
            .optional_query_value_as::<Uuid>("knowledgeBaseId")?
            .ok_or_else(|| {
                BootError::BadRequest("knowledgeBaseId query parameter is required".into())
            })?;
        match self
            .bus
            .execute(ListKnowledgeDocuments {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                knowledge_base_id,
                limit: Some(document_list_limit(&request)?),
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(records) => BootResponse::json(
                &records
                    .into_iter()
                    .map(KnowledgeDocumentResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-documents/{document_id}",
        raw
    )]
    async fn get_knowledge_document(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetKnowledgeDocument {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                document_id: request.param_as::<Uuid>("document_id")?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(record) => BootResponse::json(&KnowledgeDocumentResponse::from(record)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-documents/{document_id}/chunks",
        raw
    )]
    async fn list_knowledge_chunks(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListKnowledgeChunks {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                document_id: request.param_as::<Uuid>("document_id")?,
                limit: Some(chunk_list_limit(&request)?),
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(records) => BootResponse::json(
                &records
                    .into_iter()
                    .map(KnowledgeChunkResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-chunks/{chunk_id}",
        raw
    )]
    async fn get_knowledge_chunk(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetKnowledgeChunk {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                chunk_id: request.param_as::<Uuid>("chunk_id")?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(record) => BootResponse::json(&KnowledgeChunkResponse::from(record)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-index-revisions",
        raw
    )]
    async fn list_knowledge_index_revisions(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let knowledge_base_revision_id = request
            .optional_query_value_as::<Uuid>("knowledgeBaseRevisionId")?
            .ok_or_else(|| {
                BootError::BadRequest("knowledgeBaseRevisionId query parameter is required".into())
            })?;
        match self
            .bus
            .execute(ListKnowledgeIndexRevisions {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                knowledge_base_revision_id,
                limit: Some(index_revision_list_limit(&request)?),
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(records) => BootResponse::json(
                &records
                    .into_iter()
                    .map(KnowledgeIndexRevisionResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-retrieval-policy-revisions",
        raw
    )]
    async fn list_knowledge_retrieval_policy_revisions(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let knowledge_base_revision_id = request
            .optional_query_value_as::<Uuid>("knowledgeBaseRevisionId")?
            .ok_or_else(|| {
                BootError::BadRequest("knowledgeBaseRevisionId query parameter is required".into())
            })?;
        match self
            .bus
            .execute(ListKnowledgeRetrievalPolicyRevisions {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                knowledge_base_revision_id,
                limit: Some(retrieval_policy_revision_list_limit(&request)?),
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(records) => BootResponse::json(
                &records
                    .into_iter()
                    .map(KnowledgeRetrievalPolicyRevisionResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/external-knowledge-bindings",
        raw
    )]
    async fn list_external_knowledge_bindings(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let knowledge_base_id = request
            .optional_query_value_as::<Uuid>("knowledgeBaseId")?
            .ok_or_else(|| {
                BootError::BadRequest("knowledgeBaseId query parameter is required".into())
            })?;
        match self
            .bus
            .execute(ListExternalKnowledgeBindings {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                knowledge_base_id,
                limit: Some(external_binding_list_limit(&request)?),
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(records) => BootResponse::json(
                &records
                    .into_iter()
                    .map(ExternalKnowledgeBindingResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-index-revisions/{index_revision_id}",
        raw
    )]
    async fn get_knowledge_index_revision(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetKnowledgeIndexRevision {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                index_revision_id: request.param_as::<Uuid>("index_revision_id")?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(record) => BootResponse::json(&KnowledgeIndexRevisionResponse::from(record)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/knowledge-retrieval-policy-revisions/{policy_revision_id}",
        raw
    )]
    async fn get_knowledge_retrieval_policy_revision(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetKnowledgeRetrievalPolicyRevision {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                policy_revision_id: request.param_as::<Uuid>("policy_revision_id")?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(record) => {
                BootResponse::json(&KnowledgeRetrievalPolicyRevisionResponse::from(record))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/external-knowledge-bindings/{binding_id}",
        raw
    )]
    async fn get_external_knowledge_binding(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetExternalKnowledgeBinding {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                binding_id: request.param_as::<Uuid>("binding_id")?,
                access: knowledge_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(record) => BootResponse::json(&ExternalKnowledgeBindingResponse::from(record)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn base_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

fn pipeline_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

fn document_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_KNOWLEDGE_DOCUMENT_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_KNOWLEDGE_DOCUMENT_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_KNOWLEDGE_DOCUMENT_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

fn chunk_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_KNOWLEDGE_CHUNK_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

fn index_revision_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_KNOWLEDGE_INDEX_REVISION_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_KNOWLEDGE_INDEX_REVISION_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_KNOWLEDGE_INDEX_REVISION_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

fn retrieval_policy_revision_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_KNOWLEDGE_RETRIEVAL_POLICY_REVISION_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_KNOWLEDGE_RETRIEVAL_POLICY_REVISION_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_KNOWLEDGE_RETRIEVAL_POLICY_REVISION_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

fn external_binding_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_EXTERNAL_KNOWLEDGE_BINDING_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_EXTERNAL_KNOWLEDGE_BINDING_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_EXTERNAL_KNOWLEDGE_BINDING_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

#[cfg(test)]
mod nest_macro_knowledge_controller_tests {
    use super::*;

    #[test]
    fn knowledge_controllers_register_scoped_routes_via_nest_macros() {
        let commands =
            knowledge_commands_controller(Arc::new(CommandBus::new())).expect("knowledge commands");
        assert_eq!(commands.prefix(), "/organizations");
        assert_eq!(commands.routes().len(), 9);
        assert!(commands.metadata().get(AUTH_SCOPES_METADATA).is_some());

        let queries =
            knowledge_queries_controller(Arc::new(QueryBus::new())).expect("knowledge queries");
        assert_eq!(queries.prefix(), "/organizations");
        assert_eq!(queries.routes().len(), 14);
        assert_eq!(
            queries.routes()[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/knowledge-bases"
        );
    }
}
