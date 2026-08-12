use super::tool_result;
use crate::modules::executions::presentation::{
    ExecutionTemplateMutationResponse, ExecutionTemplateRevisionResponse,
};
use crate::modules::executions::{
    CreateExecutionTemplateCommand, GetExecutionTemplate, ListExecutionTemplates,
};
use crate::modules::shared_kernel::domain::{
    ExecutionTemplateId, ExecutionTemplateRevisionId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateExecutionTemplateArguments {
    project_id: Uuid,
    definition_acl: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListExecutionTemplatesArguments {
    project_id: Uuid,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetExecutionTemplateArguments {
    project_id: Uuid,
    template_id: Uuid,
    revision_id: Uuid,
}

pub async fn create(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateExecutionTemplateArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateExecutionTemplateCommand {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            definition_acl: arguments.definition_acl,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ExecutionTemplateMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListExecutionTemplatesArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListExecutionTemplates {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            limit: arguments.limit,
        })
        .await?
    {
        Ok(revisions) => tool_result::success(
            200,
            revisions
                .into_iter()
                .map(ExecutionTemplateRevisionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: GetExecutionTemplateArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetExecutionTemplate {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            template_id: ExecutionTemplateId::from_uuid(arguments.template_id),
            revision_id: ExecutionTemplateRevisionId::from_uuid(arguments.revision_id),
        })
        .await?
    {
        Ok(revision) => tool_result::success(
            200,
            ExecutionTemplateRevisionResponse::from(revision),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
