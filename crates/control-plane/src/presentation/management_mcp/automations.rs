use super::tool_result;
use crate::access_projection::automation_access;
use crate::modules::automations::{
    AutomationDefinitionResponse, AutomationRevisionResponse, AutomationWebhookEndpointResponse,
    ChangeAuthorizedAutomationWebhookEndpoint, CreateAuthorizedAutomationWebhookEndpoint,
    EndpointLifecycleAction, GetAuthorizedAutomationDefinition, GetAuthorizedAutomationRevision,
    GetAuthorizedAutomationWebhookEndpoint, ListAuthorizedAutomationDefinitions,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CommandBus, QueryBus, Result};
use a3s_cloud_contracts::AutomationWebhookSecretReferenceV1;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateAutomationWebhookEndpointArguments {
    project_id: Uuid,
    environment_id: Uuid,
    endpoint_id: Uuid,
    endpoint_key: String,
    signing_secret: AutomationWebhookSecretReferenceV1,
    max_body_bytes: u64,
    automation_id: Uuid,
    revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationWebhookEndpointArguments {
    project_id: Uuid,
    environment_id: Uuid,
    endpoint_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeAutomationWebhookEndpointArguments {
    project_id: Uuid,
    environment_id: Uuid,
    endpoint_id: Uuid,
    expected_generation: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListAutomationDefinitionsArguments {
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationDefinitionArguments {
    automation_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationRevisionArguments {
    automation_id: Uuid,
    revision_id: Uuid,
}

pub async fn create_webhook_endpoint(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: CreateAutomationWebhookEndpointArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateAuthorizedAutomationWebhookEndpoint {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            endpoint_id: arguments.endpoint_id,
            endpoint_key: arguments.endpoint_key,
            signing_secret: arguments.signing_secret,
            max_body_bytes: arguments.max_body_bytes,
            automation_id: arguments.automation_id,
            revision_id: arguments.revision_id,
            access: automation_access(&resource_access),
            created_at: Utc::now(),
        })
        .await?
    {
        Ok(record) => tool_result::success(
            201,
            AutomationWebhookEndpointResponse::from(record),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

async fn change_webhook_endpoint(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: ChangeAutomationWebhookEndpointArguments,
    action: EndpointLifecycleAction,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ChangeAuthorizedAutomationWebhookEndpoint {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            endpoint_id: arguments.endpoint_id,
            expected_generation: arguments.expected_generation,
            action,
            access: automation_access(&resource_access),
            changed_at: Utc::now(),
        })
        .await?
    {
        Ok(record) => tool_result::success(
            200,
            AutomationWebhookEndpointResponse::from(record),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn disable_webhook_endpoint(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: ChangeAutomationWebhookEndpointArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    change_webhook_endpoint(
        bus,
        organization_id,
        arguments,
        EndpointLifecycleAction::Disable,
        resource_access,
        request_id,
    )
    .await
}

pub async fn enable_webhook_endpoint(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: ChangeAutomationWebhookEndpointArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    change_webhook_endpoint(
        bus,
        organization_id,
        arguments,
        EndpointLifecycleAction::Enable,
        resource_access,
        request_id,
    )
    .await
}

pub async fn revoke_webhook_endpoint(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: ChangeAutomationWebhookEndpointArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    change_webhook_endpoint(
        bus,
        organization_id,
        arguments,
        EndpointLifecycleAction::Revoke,
        resource_access,
        request_id,
    )
    .await
}

pub async fn get_webhook_endpoint(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: AutomationWebhookEndpointArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetAuthorizedAutomationWebhookEndpoint {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            endpoint_id: arguments.endpoint_id,
            access: automation_access(&resource_access),
        })
        .await?
    {
        Ok(record) => tool_result::success(
            200,
            AutomationWebhookEndpointResponse::from(record),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_definitions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListAutomationDefinitionsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListAuthorizedAutomationDefinitions {
            organization_id,
            limit: Some(arguments.limit),
            access: automation_access(&resource_access),
        })
        .await?
    {
        Ok(definitions) => tool_result::success(
            200,
            definitions
                .into_iter()
                .map(AutomationDefinitionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_definition(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: AutomationDefinitionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetAuthorizedAutomationDefinition {
            organization_id,
            automation_id: arguments.automation_id,
            access: automation_access(&resource_access),
        })
        .await?
    {
        Ok(definition) => tool_result::success(
            200,
            AutomationDefinitionResponse::from(definition),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_revision(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: AutomationRevisionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetAuthorizedAutomationRevision {
            organization_id,
            automation_id: arguments.automation_id,
            revision_id: arguments.revision_id,
            access: automation_access(&resource_access),
        })
        .await?
    {
        Ok(revision) => tool_result::success(
            200,
            AutomationRevisionResponse::from(revision),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
