use crate::modules::edge::application::CreateGatewayScope;
use crate::modules::edge::domain::GatewayRolloutPolicy;
use crate::modules::edge::presentation::dto::{CreateGatewayScopeRequest, GatewayScopeResponse};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, OrganizationId, ProjectId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn gateway_scope_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ROUTE_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/gateway-scopes",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: CreateGatewayScopeRequest = request.json_with_content_type()?;
                    let (primary_node_id, member_node_ids, min_ready, max_unavailable) =
                        body.members().map_err(BootError::BadRequest)?;
                    let member_node_ids = member_node_ids
                        .into_iter()
                        .map(NodeId::from_uuid)
                        .collect::<Vec<_>>();
                    let rollout_policy = GatewayRolloutPolicy::new(
                        min_ready,
                        max_unavailable,
                        member_node_ids.len(),
                    )
                    .map_err(BootError::BadRequest)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateGatewayScope {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            node_id: NodeId::from_uuid(primary_node_id),
                            member_node_ids,
                            rollout_policy,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &GatewayScopeResponse::from(result.scope),
                        ),
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
