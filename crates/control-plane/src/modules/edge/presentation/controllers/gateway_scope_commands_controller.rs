use crate::access_projection::edge_access;
use crate::modules::edge::application::CreateGatewayScope;
use crate::modules::edge::domain::GatewayRolloutPolicy;
use crate::modules::edge::presentation::dto::{
    CreateGatewayScopeRequest, GatewayScopeMutationResponse,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, OrganizationId, ProjectId};
use crate::presentation::{OrganizationTenantGuard, resource_access_evaluator, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_identity;

pub fn gateway_scope_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(GatewayScopeCommandsController { bus }).controller()
}

#[derive(Debug, Clone)]
struct GatewayScopeCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::ROUTE_WRITE])]
impl GatewayScopeCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/gateway-scopes",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateGatewayScopeRequest = request.json_with_content_type()?;
        let (primary_node_id, member_node_ids, min_ready, max_unavailable) =
            body.members().map_err(BootError::BadRequest)?;
        let member_node_ids = member_node_ids
            .into_iter()
            .map(NodeId::from_uuid)
            .collect::<Vec<_>>();
        let rollout_policy =
            GatewayRolloutPolicy::new(min_ready, max_unavailable, member_node_ids.len())
                .map_err(BootError::BadRequest)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let access = edge_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(CreateGatewayScope {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                access,
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
                &GatewayScopeMutationResponse::new(result.scope, result.replayed),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_gateway_scope_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn gateway_scope_commands_controller_registers_scoped_guarded_post_via_nest_macros() {
        let controller = gateway_scope_commands_controller(Arc::new(CommandBus::new()))
            .expect("gateway scope nest command controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/gateway-scopes"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::ROUTE_WRITE])
        );
    }
}
