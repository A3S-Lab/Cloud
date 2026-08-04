use crate::modules::agents::application::{CreateAgentConversation, StartAgentExecution};
use crate::modules::agents::presentation::dto::{
    AgentConversationMutationResponse, AgentExecutionMutationResponse, StartAgentExecutionRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AssetId, AssetReleaseId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn agent_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let start_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::EXECUTION_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/agent-conversations",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateAgentConversation {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
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
                                &AgentConversationMutationResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/agent-conversations/{conversation_id}/executions",
            move |request: BootRequest| {
                let bus = Arc::clone(&start_bus);
                async move {
                    let body: StartAgentExecutionRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(StartAgentExecution {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            conversation_id: AgentConversationId::from_uuid(
                                request.param_as::<Uuid>("conversation_id")?,
                            ),
                            agent_asset_id: AssetId::from_uuid(body.agent_asset_id),
                            agent_asset_release_id: AssetReleaseId::from_uuid(
                                body.agent_asset_release_id,
                            ),
                            input: body.input,
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
                                &AgentExecutionMutationResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    let request_id = request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })?;
    Ok((idempotency_key, request_id))
}
