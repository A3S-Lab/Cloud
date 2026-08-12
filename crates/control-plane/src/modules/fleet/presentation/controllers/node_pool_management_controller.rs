use crate::modules::fleet::application::{ManageNodePool, NodePoolMutation};
use crate::modules::fleet::presentation::dto::{
    AddNodePoolMembersRequest, CancelNodePoolMaintenanceRequest, CreateNodePoolRequest,
    NodePoolResponse, RequestNodePoolMemberRemovalRequest, ScheduleNodePoolMaintenanceRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::domain::{NodeId, NodePoolId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn node_pool_management_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let add_bus = Arc::clone(&bus);
    let removal_bus = Arc::clone(&bus);
    let schedule_bus = Arc::clone(&bus);
    let cancel_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::NODE_WRITE])?
        .post(
            "/{organization_id}/node-pools",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: CreateNodePoolRequest = request.json_with_content_type()?;
                    execute(
                        bus,
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
            },
        )?
        .post(
            "/{organization_id}/node-pools/{node_pool_id}/members",
            move |request: BootRequest| {
                let bus = Arc::clone(&add_bus);
                async move {
                    let body: AddNodePoolMembersRequest = request.json_with_content_type()?;
                    let node_pool_id =
                        NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
                    execute(
                        bus,
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
            },
        )?
        .post(
            "/{organization_id}/node-pools/{node_pool_id}/members/removal",
            move |request: BootRequest| {
                let bus = Arc::clone(&removal_bus);
                async move {
                    let body: RequestNodePoolMemberRemovalRequest =
                        request.json_with_content_type()?;
                    let node_pool_id =
                        NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
                    execute(
                        bus,
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
            },
        )?
        .post(
            "/{organization_id}/node-pools/{node_pool_id}/maintenance",
            move |request: BootRequest| {
                let bus = Arc::clone(&schedule_bus);
                async move {
                    let body: ScheduleNodePoolMaintenanceRequest =
                        request.json_with_content_type()?;
                    let node_pool_id =
                        NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
                    execute(
                        bus,
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
            },
        )?
        .post(
            "/{organization_id}/node-pools/{node_pool_id}/maintenance/cancel",
            move |request: BootRequest| {
                let bus = Arc::clone(&cancel_bus);
                async move {
                    let body: CancelNodePoolMaintenanceRequest =
                        request.json_with_content_type()?;
                    let node_pool_id =
                        NodePoolId::from_uuid(request.param_as::<Uuid>("node_pool_id")?);
                    execute(
                        bus,
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
            },
        )
}

async fn execute(
    bus: Arc<CommandBus>,
    request: &BootRequest,
    node_pool_id: NodePoolId,
    mutation: NodePoolMutation,
    created: bool,
) -> Result<BootResponse> {
    let organization_id = OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
    let (idempotency_key, request_id) = request_identity(request)?;
    let resource_access = resource_access_evaluator(&request.require_auth_principal()?)?;
    match bus
        .execute(ManageNodePool {
            organization_id,
            node_pool_id,
            mutation,
            resource_access,
            idempotency_key,
            request_id,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => {
            let status = if created && !result.replayed {
                201
            } else {
                200
            };
            BootResponse::json_with_status(status, &NodePoolResponse::from(result))
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
