use crate::modules::fleet::application::{ChangeNodeState, IssueEnrollmentToken};
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::fleet::presentation::dto::{
    ChangeNodeStateRequest, EnrollmentTokenResponse, IssueEnrollmentTokenRequest, NodeResponse,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

pub fn node_management_controller(
    bus: Arc<CommandBus>,
    heartbeat_timeout: Duration,
) -> Result<ControllerDefinition> {
    let ready_bus = Arc::clone(&bus);
    let drain_bus = Arc::clone(&bus);
    let revoke_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::NODE_WRITE])?
        .post(
            "/{organization_id}/enrollment-tokens",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: IssueEnrollmentTokenRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(IssueEnrollmentToken {
                            organization_id,
                            name: body.name,
                            token_secret: body.token,
                            expires_at: body.expires_at,
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
                                &EnrollmentTokenResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/nodes/{node_id}/actions/ready",
            move |request: BootRequest| {
                change_state(
                    Arc::clone(&ready_bus),
                    request,
                    NodeState::Ready,
                    heartbeat_timeout,
                )
            },
        )?
        .post(
            "/{organization_id}/nodes/{node_id}/actions/drain",
            move |request: BootRequest| {
                change_state(
                    Arc::clone(&drain_bus),
                    request,
                    NodeState::Draining,
                    heartbeat_timeout,
                )
            },
        )?
        .post(
            "/{organization_id}/nodes/{node_id}/actions/revoke",
            move |request: BootRequest| {
                change_state(
                    Arc::clone(&revoke_bus),
                    request,
                    NodeState::Revoked,
                    heartbeat_timeout,
                )
            },
        )
}

async fn change_state(
    bus: Arc<CommandBus>,
    request: BootRequest,
    state: NodeState,
    heartbeat_timeout: Duration,
) -> Result<BootResponse> {
    let body: ChangeNodeStateRequest = request.json_with_content_type()?;
    let organization_id = OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
    let node_id = NodeId::from_uuid(request.param_as::<Uuid>("node_id")?);
    let (idempotency_key, request_id) = request_identity(&request)?;
    match bus
        .execute(ChangeNodeState {
            organization_id,
            node_id,
            state,
            expected_version: body.expected_version,
            idempotency_key,
            request_id,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => {
            let availability = result.node.availability_at(Utc::now(), heartbeat_timeout);
            BootResponse::json(&NodeResponse::from((result, availability)))
        }
        Err(error) => application_error_response(error, request_id),
    }
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
