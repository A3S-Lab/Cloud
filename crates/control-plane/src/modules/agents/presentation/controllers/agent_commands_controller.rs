use super::request::{credential_actor, expected_version, request_identity};
use crate::modules::agents::application::{
    CancelAgentExecution, CaptureAgentExecutionCheckpoint, CreateAgentConversation,
    DecideAgentApprovalCheckpoint, ForkAgentExecution, StartAgentExecution,
};
use crate::modules::agents::presentation::dto::{
    AgentApprovalCheckpointMutationResponse, AgentApprovalDecisionRequest,
    AgentConversationMutationResponse, AgentExecutionCheckpointMutationResponse,
    AgentExecutionMutationResponse, CaptureAgentExecutionCheckpointRequest,
    ForkAgentExecutionRequest, StartAgentExecutionRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{
    AgentApprovalCheckpointId, AgentConversationId, AgentExecutionCheckpointId, AgentExecutionId,
    AssetId, AssetReleaseId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn agent_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let start_bus = Arc::clone(&bus);
    let cancel_bus = Arc::clone(&bus);
    let capture_checkpoint_bus = Arc::clone(&bus);
    let fork_execution_bus = Arc::clone(&bus);
    let decide_approval_bus = Arc::clone(&bus);
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
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/agent-conversations/{conversation_id}/executions",
                move |request: BootRequest| {
                    let bus = Arc::clone(&start_bus);
                    async move {
                        let body: StartAgentExecutionRequest = request.json_with_content_type()?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        match bus
                            .execute(StartAgentExecution {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                conversation_id: AgentConversationId::from_uuid(
                                    request.param_as::<Uuid>("conversation_id")?,
                                ),
                                resource_access,
                                agent_asset_id: AssetId::from_uuid(body.agent_asset_id),
                                agent_asset_release_id: AssetReleaseId::from_uuid(
                                    body.agent_asset_release_id,
                                ),
                                provider_kind: body.provider_kind,
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
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/agent-executions/{execution_id}/cancel",
                move |request: BootRequest| {
                    let bus = Arc::clone(&cancel_bus);
                    async move {
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        match bus
                            .execute(CancelAgentExecution {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                execution_id: AgentExecutionId::from_uuid(
                                    request.param_as::<Uuid>("execution_id")?,
                                ),
                                resource_access,
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
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/agent-executions/{execution_id}/checkpoints",
                move |request: BootRequest| {
                    let bus = Arc::clone(&capture_checkpoint_bus);
                    async move {
                        let body: CaptureAgentExecutionCheckpointRequest =
                            request.json_with_content_type()?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        match bus
                            .execute(CaptureAgentExecutionCheckpoint {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                execution_id: AgentExecutionId::from_uuid(
                                    request.param_as::<Uuid>("execution_id")?,
                                ),
                                resource_access,
                                through_event_sequence: body.through_event_sequence,
                                idempotency_key,
                                request_id,
                            })
                            .await?
                        {
                            Ok(result) => {
                                let status = if result.replayed { 200 } else { 201 };
                                BootResponse::json_with_status(
                                    status,
                                    &AgentExecutionCheckpointMutationResponse::from(result),
                                )
                            }
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/agent-executions/{execution_id}/checkpoints/{checkpoint_id}/fork",
                move |request: BootRequest| {
                    let bus = Arc::clone(&fork_execution_bus);
                    async move {
                        let body: ForkAgentExecutionRequest = request.json_with_content_type()?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        match bus
                            .execute(ForkAgentExecution {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                parent_execution_id: AgentExecutionId::from_uuid(
                                    request.param_as::<Uuid>("execution_id")?,
                                ),
                                checkpoint_id: AgentExecutionCheckpointId::from_uuid(
                                    request.param_as::<Uuid>("checkpoint_id")?,
                                ),
                                resource_access,
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
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/agent-executions/{execution_id}/approval-checkpoints/{checkpoint_id}/decision",
                move |request: BootRequest| {
                    let bus = Arc::clone(&decide_approval_bus);
                    async move {
                        let body: AgentApprovalDecisionRequest =
                            request.json_with_content_type()?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        let actor = credential_actor(&request)?;
                        match bus
                            .execute(DecideAgentApprovalCheckpoint {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                execution_id: AgentExecutionId::from_uuid(
                                    request.param_as::<Uuid>("execution_id")?,
                                ),
                                checkpoint_id: AgentApprovalCheckpointId::from_uuid(
                                    request.param_as::<Uuid>("checkpoint_id")?,
                                ),
                                expected_version: expected_version(&request)?,
                                outcome: body.outcome.into(),
                                reason: body.reason,
                                resource_access,
                                actor_principal_id: actor.principal_id,
                                credential_id: actor.credential_id,
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
                                    &AgentApprovalCheckpointMutationResponse::from(result),
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
