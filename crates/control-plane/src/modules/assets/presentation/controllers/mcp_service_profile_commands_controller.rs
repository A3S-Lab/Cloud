use super::asset_request::asset_release_ids;
use crate::modules::assets::application::commands::BindMcpServiceProfile;
use crate::modules::assets::domain::MCP_SERVICE_PROFILE_MAX_ACL_BYTES;
use crate::modules::assets::presentation::dto::McpServiceProfileResponse;
use crate::presentation::{
    application_error_response, asset_access, bounded_acl_document,
    organization_tenant_asset_write_controller, request_identity, resource_access_evaluator,
    with_deferred_resource_scope, DeferredResourceScope,
};
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
};
use std::sync::Arc;

pub fn mcp_service_profile_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new("/organizations")?
        .route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/mcp-service-profile",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (organization_id, asset_id, asset_release_id) =
                        asset_release_ids(&request)?;
                    let access = asset_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let acl = bounded_acl_document(
                        &request,
                        MCP_SERVICE_PROFILE_MAX_ACL_BYTES,
                        "MCP Service profile",
                    )?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(BindMcpServiceProfile {
                            organization_id,
                            asset_id,
                            asset_release_id,
                            access,
                            acl,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(write) => {
                            let status = if write.replayed { 200 } else { 201 };
                            BootResponse::json_with_status(
                                status,
                                &McpServiceProfileResponse::from(write),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Any,
    )?)?;
    organization_tenant_asset_write_controller(controller)
}
