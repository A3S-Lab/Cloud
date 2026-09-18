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
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::{
    application_error_response, organization_tenant_automation_write_controller,
    organization_tenant_cloud_read_controller, request_id, resource_access_evaluator,
};
use a3s_boot::{
    controller, get, post, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn automation_webhook_lifecycle_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let controller = Arc::new(AutomationWebhookLifecycleCommandsController { bus }).controller()?;
    organization_tenant_automation_write_controller(controller)
}

pub fn automation_management_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let controller = Arc::new(AutomationManagementQueriesController { bus }).controller()?;
    organization_tenant_cloud_read_controller(controller)
}

#[derive(Debug, Clone)]
struct AutomationWebhookLifecycleCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct AutomationManagementQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
impl AutomationWebhookLifecycleCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateAutomationWebhookEndpointRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(CreateAuthorizedAutomationWebhookEndpoint {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
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

    #[post("/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/disable", raw)]
    async fn disable(&self, request: BootRequest) -> Result<BootResponse> {
        change_endpoint(
            Arc::clone(&self.bus),
            request,
            EndpointLifecycleAction::Disable,
        )
        .await
    }

    #[post("/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/enable", raw)]
    async fn enable(&self, request: BootRequest) -> Result<BootResponse> {
        change_endpoint(
            Arc::clone(&self.bus),
            request,
            EndpointLifecycleAction::Enable,
        )
        .await
    }

    #[post("/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/revoke", raw)]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        change_endpoint(
            Arc::clone(&self.bus),
            request,
            EndpointLifecycleAction::Revoke,
        )
        .await
    }
}

async fn change_endpoint(
    bus: Arc<CommandBus>,
    request: BootRequest,
    action: EndpointLifecycleAction,
) -> Result<BootResponse> {
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

#[controller("/organizations")]
impl AutomationManagementQueriesController {
    #[get("/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}", raw)]
    async fn get_endpoint(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetAuthorizedAutomationWebhookEndpoint {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
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

    #[get("/{organization_id}/automation-definitions", raw)]
    async fn list_definitions(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = parse_limit(&request)?;
        match self
            .bus
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

    #[get("/{organization_id}/automation-definitions/{automation_id}", raw)]
    async fn get_definition(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get("/{organization_id}/automation-definitions/{automation_id}/revisions/{revision_id}", raw)]
    async fn get_revision(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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

#[cfg(test)]
mod nest_macro_automation_management_controller_tests {
    use super::*;

    #[test]
    fn automation_management_controllers_register_scoped_routes_via_nest_macros() {
        let commands =
            automation_webhook_lifecycle_commands_controller(Arc::new(CommandBus::new()))
                .expect("commands");
        let queries =
            automation_management_queries_controller(Arc::new(QueryBus::new())).expect("queries");
        assert_routes_contain(
            &commands,
            &[
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints",
                ),
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/disable",
                ),
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/enable",
                ),
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}/revoke",
                ),
            ],
        );
        assert_routes_contain(
            &queries,
            &[
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/automation-webhook-endpoints/{endpoint_id}",
                ),
                (
                    "GET",
                    "/organizations/{organization_id}/automation-definitions",
                ),
                (
                    "GET",
                    "/organizations/{organization_id}/automation-definitions/{automation_id}",
                ),
                (
                    "GET",
                    "/organizations/{organization_id}/automation-definitions/{automation_id}/revisions/{revision_id}",
                ),
            ],
        );
    }

    fn assert_routes_contain(controller: &ControllerDefinition, expected: &[(&str, &str)]) {
        let routes = controller.routes();
        for (method, path) in expected {
            assert!(
                routes.iter().any(|route| {
                    route.method().as_str().eq_ignore_ascii_case(method) && route.path() == *path
                }),
                "missing {method} {path}; have {:?}",
                routes
                    .iter()
                    .map(|route| format!("{} {}", route.method().as_str(), route.path()))
                    .collect::<Vec<_>>(),
            );
        }
    }
}
