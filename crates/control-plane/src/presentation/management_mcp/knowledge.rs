use super::{arguments, tool_result};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::knowledge::{
    AppendKnowledgeBaseCommand, CreateKnowledgeBaseCommand, CreateKnowledgePipelineCommand,
    GetKnowledgeBase, GetKnowledgePipeline, KnowledgeBaseMutationResponse, KnowledgeBaseResponse,
    KnowledgePipelineMutationResponse, KnowledgePipelineResponse, ListKnowledgeBases,
    ListKnowledgePipelines, PublishKnowledgePipelineCommand,
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
