use super::dto::{
    AutomationDefinitionResponse, AutomationRevisionResponse, AutomationWebhookEndpointResponse,
    ChangeAutomationWebhookEndpointRequest, CreateAutomationWebhookEndpointRequest,
};
use crate::access_projection::automation_access;
use crate::modules::automations::application::{
    ChangeAuthorizedAutomationWebhookEndpoint, CreateAuthorizedAutomationWebhookEndpoint,
    EndpointLifecycleAction, GetAuthorizedAutomationDefinition, GetAuthorizedAutomationRevision,
    GetAuthorizedAutomationWebhookEndpoint, ListAuthorizedAutomationDefinitions,
    DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT, MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT,
};
use crate::modules::identity::presentation::resource_access_evaluator;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::{
    application_error_response, organization_tenant_automation_write_controller,
    organization_tenant_cloud_read_controller, request_id,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

const WEBHOOK_ENDPOINT_COLLECTION: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints";
const WEBHOOK_ENDPOINT_ITEM: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}";
const WEBHOOK_ENDPOINT_DISABLE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/disable";
const WEBHOOK_ENDPOINT_ENABLE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/enable";
const WEBHOOK_ENDPOINT_REVOKE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/revoke";
const DEFINITION_COLLECTION: &str = "/{organization_id}/automation-definitions";
const DEFINITION_ITEM: &str = "/{organization_id}/automation-definitions/{automation_id}";
const REVISION_ITEM: &str =
    "/{organization_id}/automation-definitions/{automation_id}/revisions/{revision_id}";

pub fn automation_webhook_lifecycle_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    let disable_bus = Arc::clone(&bus);
    let enable_bus = Arc::clone(&bus);
    let revoke_bus = Arc::clone(&bus);
    organization_tenant_automation_write_controller(
        ControllerDefinition::new("/organizations")?
            .post(WEBHOOK_ENDPOINT_COLLECTION, move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateAutomationWebhookEndpointRequest =
                        request.json_with_content_type()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(CreateAuthorizedAutomationWebhookEndpoint {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            endpoint_id: body.endpoint_id,
                            endpoint_key: body.endpoint_key,
                            signing_secret: body.signing_secret,
                            max_body_bytes: body.max_body_bytes,
                            automation_id: body.automation_id,
                            revision_id: body.revision_id,
                            access: automation_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                            created_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(record) => BootResponse::json_with_status(
                            201,
                            &AutomationWebhookEndpointResponse::from(record),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            })?
            .post(WEBHOOK_ENDPOINT_DISABLE, move |request: BootRequest| {
                let bus = Arc::clone(&disable_bus);
                change_endpoint(bus, request, EndpointLifecycleAction::Disable)
            })?
            .post(WEBHOOK_ENDPOINT_ENABLE, move |request: BootRequest| {
                let bus = Arc::clone(&enable_bus);
                change_endpoint(bus, request, EndpointLifecycleAction::Enable)
            })?
            .post(WEBHOOK_ENDPOINT_REVOKE, move |request: BootRequest| {
                let bus = Arc::clone(&revoke_bus);
                change_endpoint(bus, request, EndpointLifecycleAction::Revoke)
            })?,
    )
}

async fn change_endpoint(
    bus: Arc<CommandBus>,
    request: BootRequest,
    action: EndpointLifecycleAction,
) -> a3s_boot::Result<BootResponse> {
    let body: ChangeAutomationWebhookEndpointRequest = request.json_with_content_type()?;
    let request_id = request_id(&request)?;
    match bus
        .execute(ChangeAuthorizedAutomationWebhookEndpoint {
            organization_id: OrganizationId::from_uuid(
                request.param_as::<Uuid>("organization_id")?,
            ),
            project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
            environment_id: EnvironmentId::from_uuid(
                request.param_as::<Uuid>("environment_id")?,
            ),
            endpoint_id: request.param_as::<Uuid>("endpoint_id")?,
            expected_generation: body.expected_generation,
            action,
            access: automation_access(&resource_access_evaluator(
                &request.require_auth_principal()?,
            )?),
            changed_at: Utc::now(),
        })
        .await?
    {
        Ok(record) => {
            BootResponse::json_with_status(200, &AutomationWebhookEndpointResponse::from(record))
        }
        Err(error) => application_error_response(error, request_id),
    }
}

pub fn automation_management_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let get_endpoint_bus = Arc::clone(&bus);
    let list_definitions_bus = Arc::clone(&bus);
    let get_definition_bus = Arc::clone(&bus);
    let get_revision_bus = Arc::clone(&bus);
    organization_tenant_cloud_read_controller(
        ControllerDefinition::new("/organizations")?
            .get(WEBHOOK_ENDPOINT_ITEM, move |request: BootRequest| {
                let bus = Arc::clone(&get_endpoint_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAuthorizedAutomationWebhookEndpoint {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            endpoint_id: request.param_as::<Uuid>("endpoint_id")?,
                            access: automation_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                        })
                        .await?
                    {
                        Ok(record) => BootResponse::json_with_status(
                            200,
                            &AutomationWebhookEndpointResponse::from(record),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            })?
            .get(DEFINITION_COLLECTION, move |request: BootRequest| {
                let bus = Arc::clone(&list_definitions_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let limit = parse_limit(&request)?;
                    match bus
                        .execute(ListAuthorizedAutomationDefinitions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            limit,
                            access: automation_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                        })
                        .await?
                    {
                        Ok(records) => BootResponse::json_with_status(
                            200,
                            &records
                                .into_iter()
                                .map(AutomationDefinitionResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            })?
            .get(DEFINITION_ITEM, move |request: BootRequest| {
                let bus = Arc::clone(&get_definition_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAuthorizedAutomationDefinition {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            automation_id: request.param_as::<Uuid>("automation_id")?,
                            access: automation_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                        })
                        .await?
                    {
                        Ok(record) => BootResponse::json_with_status(
                            200,
                            &AutomationDefinitionResponse::from(record),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            })?
            .get(REVISION_ITEM, move |request: BootRequest| {
                let bus = Arc::clone(&get_revision_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetAuthorizedAutomationRevision {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            automation_id: request.param_as::<Uuid>("automation_id")?,
                            revision_id: request.param_as::<Uuid>("revision_id")?,
                            access: automation_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                        })
                        .await?
                    {
                        Ok(revision) => BootResponse::json_with_status(
                            200,
                            &AutomationRevisionResponse::from(revision),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            })?,
    )
}

fn parse_limit(request: &BootRequest) -> Result<Option<usize>> {
    let Some(limit) = request.optional_query_value_as::<usize>("limit")? else {
        return Ok(None);
    };
    if limit == 0 || limit > MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT}"
        )));
    }
    let _ = DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT;
    Ok(Some(limit))
}
