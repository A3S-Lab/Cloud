use super::{
    AppendKnowledgeBaseRequest, CreateKnowledgeBaseRequest, CreateKnowledgeChunkRequest,
    CreateKnowledgeDocumentRequest, CreateKnowledgePipelineRequest, KnowledgeBaseMutationResponse,
    KnowledgeBaseResponse, KnowledgeChunkMutationResponse, KnowledgeChunkResponse,
    KnowledgeDocumentMutationResponse, KnowledgeDocumentResponse,
    KnowledgePipelineMutationResponse, KnowledgePipelineResponse, PublishKnowledgePipelineRequest,
    KNOWLEDGE_BASE_COLLECTION_ROUTE, KNOWLEDGE_BASE_ITEM_ROUTE, KNOWLEDGE_BASE_REVISION_ROUTE,
    KNOWLEDGE_CHUNK_ITEM_ROUTE, KNOWLEDGE_CONTROLLER_PREFIX,
    KNOWLEDGE_DOCUMENT_CHUNK_COLLECTION_ROUTE, KNOWLEDGE_DOCUMENT_COLLECTION_ROUTE,
    KNOWLEDGE_DOCUMENT_ITEM_ROUTE, KNOWLEDGE_PIPELINE_COLLECTION_ROUTE,
    KNOWLEDGE_PIPELINE_ITEM_ROUTE, KNOWLEDGE_PIPELINE_RELEASE_ROUTE,
};
use crate::modules::knowledge::application::{
    AppendKnowledgeBaseCommand, CreateKnowledgeBaseCommand, CreateKnowledgeChunkCommand,
    CreateKnowledgeDocumentCommand, CreateKnowledgePipelineCommand, GetKnowledgeBase,
    GetKnowledgeChunk, GetKnowledgeDocument, GetKnowledgePipeline, ListKnowledgeBases,
    ListKnowledgePipelines, PublishKnowledgePipelineCommand, DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT,
    DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT, MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
    MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
};
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::presentation::{
    actor_principal_id, application_error_response, knowledge_access,
    organization_tenant_cloud_read_controller, organization_tenant_knowledge_write_controller,
    request_id, request_identity, resource_access_evaluator,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn knowledge_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_base_bus = Arc::clone(&bus);
    let append_base_bus = Arc::clone(&bus);
    let create_pipeline_bus = Arc::clone(&bus);
    let publish_pipeline_bus = Arc::clone(&bus);
    let create_document_bus = Arc::clone(&bus);
    let create_chunk_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new(KNOWLEDGE_CONTROLLER_PREFIX)?
        .post(
            KNOWLEDGE_BASE_COLLECTION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&create_base_bus);
                async move {
                    let body: CreateKnowledgeBaseRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateKnowledgeBaseCommand {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .post(
            KNOWLEDGE_BASE_REVISION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&append_base_bus);
                async move {
                    let body: AppendKnowledgeBaseRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(AppendKnowledgeBaseCommand {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
                        Ok(result) => {
                            BootResponse::json(&KnowledgeBaseMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            KNOWLEDGE_PIPELINE_COLLECTION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&create_pipeline_bus);
                async move {
                    let body: CreateKnowledgePipelineRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateKnowledgePipelineCommand {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .post(
            KNOWLEDGE_PIPELINE_RELEASE_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&publish_pipeline_bus);
                async move {
                    let body: PublishKnowledgePipelineRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(PublishKnowledgePipelineCommand {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
                        Ok(result) => {
                            BootResponse::json(&KnowledgePipelineMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            KNOWLEDGE_DOCUMENT_COLLECTION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&create_document_bus);
                async move {
                    let body: CreateKnowledgeDocumentRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateKnowledgeDocumentCommand {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .post(
            KNOWLEDGE_DOCUMENT_CHUNK_COLLECTION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&create_chunk_bus);
                async move {
                    let body: CreateKnowledgeChunkRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateKnowledgeChunkCommand {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?;
    organization_tenant_knowledge_write_controller(controller)
}

pub fn knowledge_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bases_bus = Arc::clone(&bus);
    let get_base_bus = Arc::clone(&bus);
    let list_pipelines_bus = Arc::clone(&bus);
    let get_pipeline_bus = Arc::clone(&bus);
    let get_document_bus = Arc::clone(&bus);
    let get_chunk_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new(KNOWLEDGE_CONTROLLER_PREFIX)?
        .get(
            KNOWLEDGE_BASE_COLLECTION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bases_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListKnowledgeBases {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .get(KNOWLEDGE_BASE_ITEM_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&get_base_bus);
            async move {
                let request_id = request_id(&request)?;
                match bus
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
        })?
        .get(
            KNOWLEDGE_PIPELINE_COLLECTION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&list_pipelines_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListKnowledgePipelines {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .get(
            KNOWLEDGE_PIPELINE_ITEM_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&get_pipeline_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetKnowledgePipeline {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .get(
            KNOWLEDGE_DOCUMENT_ITEM_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&get_document_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetKnowledgeDocument {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .get(KNOWLEDGE_CHUNK_ITEM_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&get_chunk_bus);
            async move {
                let request_id = request_id(&request)?;
                match bus
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
        })?;
    organization_tenant_cloud_read_controller(controller)
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
