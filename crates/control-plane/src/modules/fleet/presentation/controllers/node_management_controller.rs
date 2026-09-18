use crate::modules::fleet::application::{ChangeNodeState, IssueEnrollmentToken};
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::fleet::presentation::dto::{
    ChangeNodeStateRequest, EnrollmentTokenResponse, IssueEnrollmentTokenRequest, NodeResponse,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use crate::presentation::{OrganizationTenantGuard, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, Result,
};
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

pub fn node_management_controller(
    bus: Arc<CommandBus>,
    heartbeat_timeout: Duration,
) -> Result<ControllerDefinition> {
    Arc::new(NodeManagementController {
        bus,
        heartbeat_timeout,
    })
    .controller()
}

#[derive(Debug, Clone)]
struct NodeManagementController {
    bus: Arc<CommandBus>,
    heartbeat_timeout: Duration,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::NODE_WRITE])]
impl NodeManagementController {
    #[post("/{organization_id}/enrollment-tokens", raw)]
    async fn issue_enrollment_token(&self, request: BootRequest) -> Result<BootResponse> {
        let body: IssueEnrollmentTokenRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
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
                BootResponse::json_with_status(status, &EnrollmentTokenResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/nodes/{node_id}/actions/ready", raw)]
    async fn ready(&self, request: BootRequest) -> Result<BootResponse> {
        self.change_state(request, NodeState::Ready).await
    }

    #[post("/{organization_id}/nodes/{node_id}/actions/drain", raw)]
    async fn drain(&self, request: BootRequest) -> Result<BootResponse> {
        self.change_state(request, NodeState::Draining).await
    }

    #[post("/{organization_id}/nodes/{node_id}/actions/revoke", raw)]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        self.change_state(request, NodeState::Revoked).await
    }
}

impl NodeManagementController {
    async fn change_state(
        &self,
        request: BootRequest,
        state: NodeState,
    ) -> Result<BootResponse> {
        let body: ChangeNodeStateRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let node_id = NodeId::from_uuid(request.param_as::<Uuid>("node_id")?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
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
                let availability =
                    result
                        .node
                        .availability_at(Utc::now(), self.heartbeat_timeout);
                BootResponse::json(&NodeResponse::from((result, availability)))
            }
            Err(error) => application_error_response(error, request_id),
        }
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

#[cfg(test)]
mod nest_macro_node_management_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn node_management_controller_registers_scoped_posts_via_nest_macros() {
        let controller = node_management_controller(
            Arc::new(CommandBus::new()),
            Duration::seconds(30),
        )
        .expect("node management nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/enrollment-tokens"
        );
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/nodes/{node_id}/actions/ready"
        );
        assert_eq!(
            routes[2].path(),
            "/organizations/{organization_id}/nodes/{node_id}/actions/drain"
        );
        assert_eq!(
            routes[3].path(),
            "/organizations/{organization_id}/nodes/{node_id}/actions/revoke"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::NODE_WRITE])
        );
    }
}
