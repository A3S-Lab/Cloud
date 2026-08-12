use super::request::{actor_principal_id, request_identity};
use crate::modules::executions::application::{
    CancelExecution, CreateExecutionCommand, CreateExecutionTemplateCommand,
};
use crate::modules::executions::presentation::dto::{
    CreateExecutionRequest, CreateExecutionTemplateRequest, ExecutionMutationResponse,
    ExecutionTemplateMutationResponse,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn execution_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let cancel_bus = Arc::clone(&bus);
    let template_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::EXECUTION_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/execution-templates",
            move |request: BootRequest| {
                let bus = Arc::clone(&template_bus);
                async move {
                    let body: CreateExecutionTemplateRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateExecutionTemplateCommand {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/executions",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: CreateExecutionRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateExecutionCommand {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            template: body.into(),
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
        )?
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
                                resource_access: resource_access_evaluator(
                                    &request.require_auth_principal()?,
                                )?,
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
