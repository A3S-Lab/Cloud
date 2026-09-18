use super::request::{actor_principal_id, request_identity};
use crate::access_projection::execution_access;
use crate::modules::executions::application::{
    CancelExecution, CreateExecutionCommand, CreateExecutionTemplateCommand,
};
use crate::modules::executions::presentation::dto::{
    CreateExecutionRequest, CreateExecutionTemplateRequest, ExecutionMutationResponse,
    ExecutionTemplateMutationResponse,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, OrganizationId, ProjectId,
};
use crate::presentation::{DeferredResourceScope, OrganizationTenantGuard, resource_access_evaluator, with_deferred_resource_scope, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result, RouteDefinition,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn execution_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    // Nest macros own create-template and create-execution; cancel stays deferred
    // project-scoped because deferred resource scope is not a Nest attribute today.
    let cancel_bus = Arc::clone(&bus);
    Arc::new(ExecutionCommandsController { bus })
        .controller()?
        .route(with_deferred_resource_scope(
            RouteDefinition::delete(
                "/{organization_id}/executions/{execution_id}",
                move |request: BootRequest| {
                    let bus = Arc::clone(&cancel_bus);
                    async move {
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(CancelExecution {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                execution_id: ExecutionId::from_uuid(
                                    request.param_as::<Uuid>("execution_id")?,
                                ),
                                access: execution_access(&resource_access_evaluator(
                                    &request.require_auth_principal()?,
                                )?),
                                idempotency_key,
                                request_id,
                                requested_at: Utc::now(),
                            })
                            .await?
                        {
                            Ok(result) => {
                                let status = if result.replayed { 200 } else { 202 };
                                BootResponse::json_with_status(
                                    status,
                                    &ExecutionMutationResponse::from(result),
                                )
                            }
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Project,
        )?)
}

#[derive(Debug, Clone)]
struct ExecutionCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::EXECUTION_WRITE])]
impl ExecutionCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/execution-templates",
        raw
    )]
    async fn create_template(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateExecutionTemplateRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateExecutionTemplateCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                access: execution_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                definition_acl: body.definition_acl,
                actor_principal_id: actor_principal_id(&request)?,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &ExecutionTemplateMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/executions",
        raw
    )]
    async fn create_execution(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateExecutionRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateExecutionCommand {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                access: execution_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                template: body.into(),
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 202 };
                BootResponse::json_with_status(status, &ExecutionMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_execution_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn execution_commands_controller_registers_creates_via_nest_macros() {
        let controller = execution_commands_controller(Arc::new(CommandBus::new()))
            .expect("execution commands nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/execution-templates"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Delete
                && route.path()
                    == "/organizations/{organization_id}/executions/{execution_id}"
        }));
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::EXECUTION_WRITE])
        );
    }
}
