use crate::access_projection::fleet_access;
use crate::modules::fleet::application::{ManageNodePool, NodePoolMutation};
use crate::modules::fleet::presentation::dto::{
    AddNodePoolMembersRequest, CancelNodePoolMaintenanceRequest, CreateNodePoolRequest,
    NodePoolResponse, RequestNodePoolMemberRemovalRequest, ScheduleNodePoolMaintenanceRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{NodeId, NodePoolId, OrganizationId};
use crate::presentation::{OrganizationTenantGuard, resource_access_evaluator, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn node_pool_management_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(NodePoolManagementController { bus }).controller()
}

#[derive(Debug, Clone)]
struct NodePoolManagementController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::NODE_WRITE])]
impl NodePoolManagementController {
    #[post("/{organization_id}/node-pools", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateNodePoolRequest = request.json_with_content_type()?;
        self.execute(
            &request,
            NodePoolId::new(),
            NodePoolMutation::Create {
                name: body.name,
                member_node_ids: body
                    .member_node_ids
                    .into_iter()
                    .map(NodeId::from_uuid)
                    .collect(),
            },
            true,
        )
        .await
    }

    #[post("/{organization_id}/node-pools/{node_pool_id}/members", raw)]
    async fn add_members(&self, request: BootRequest) -> Result<BootResponse> {
        let body: AddNodePoolMembersRequest = request.json_with_content_type()?;
        let node_pool_id = NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
        self.execute(
            &request,
            node_pool_id,
            NodePoolMutation::AddMembers {
                expected_version: body.expected_version,
                member_node_ids: body
                    .member_node_ids
                    .into_iter()
                    .map(NodeId::from_uuid)
                    .collect(),
            },
            false,
        )
        .await
    }

    #[post("/{organization_id}/node-pools/{node_pool_id}/members/removal", raw)]
    async fn request_member_removal(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RequestNodePoolMemberRemovalRequest = request.json_with_content_type()?;
        let node_pool_id = NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
        self.execute(
            &request,
            node_pool_id,
            NodePoolMutation::RequestMemberRemoval {
                expected_version: body.expected_version,
                member_node_ids: body
                    .member_node_ids
                    .into_iter()
                    .map(NodeId::from_uuid)
                    .collect(),
            },
            false,
        )
        .await
    }

    #[post("/{organization_id}/node-pools/{node_pool_id}/maintenance", raw)]
    async fn schedule_maintenance(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ScheduleNodePoolMaintenanceRequest = request.json_with_content_type()?;
        let node_pool_id = NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
        self.execute(
            &request,
            node_pool_id,
            NodePoolMutation::ScheduleMaintenance {
                expected_version: body.expected_version,
                target_node_ids: body
                    .target_node_ids
                    .into_iter()
                    .map(NodeId::from_uuid)
                    .collect(),
                starts_at: body.starts_at,
                ends_at: body.ends_at,
                reason: body.reason,
            },
            false,
        )
        .await
    }

    #[post("/{organization_id}/node-pools/{node_pool_id}/maintenance/cancel", raw)]
    async fn cancel_maintenance(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CancelNodePoolMaintenanceRequest = request.json_with_content_type()?;
        let node_pool_id = NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
        self.execute(
            &request,
            node_pool_id,
            NodePoolMutation::CancelMaintenance {
                expected_version: body.expected_version,
                maintenance_generation: body.maintenance_generation,
            },
            false,
        )
        .await
    }
}

impl NodePoolManagementController {
    async fn execute(
        &self,
        request: &BootRequest,
        node_pool_id: NodePoolId,
        mutation: NodePoolMutation,
        created: bool,
    ) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let (idempotency_key, request_id) = request_identity(request)?;
        let access = fleet_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ManageNodePool {
                organization_id,
                node_pool_id,
                mutation,
                access,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => {
                let status = if created && !result.replayed { 201 } else { 200 };
                BootResponse::json_with_status(status, &NodePoolResponse::from(result))
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
mod nest_macro_node_pool_management_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn node_pool_management_controller_registers_scoped_posts_via_nest_macros() {
        let controller = node_pool_management_controller(Arc::new(CommandBus::new()))
            .expect("node pool management nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 5);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(routes[0].path(), "/organizations/{organization_id}/node-pools");
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/node-pools/{node_pool_id}/members"
        );
        assert_eq!(
            routes[2].path(),
            "/organizations/{organization_id}/node-pools/{node_pool_id}/members/removal"
        );
        assert_eq!(
            routes[3].path(),
            "/organizations/{organization_id}/node-pools/{node_pool_id}/maintenance"
        );
        assert_eq!(
            routes[4].path(),
            "/organizations/{organization_id}/node-pools/{node_pool_id}/maintenance/cancel"
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
