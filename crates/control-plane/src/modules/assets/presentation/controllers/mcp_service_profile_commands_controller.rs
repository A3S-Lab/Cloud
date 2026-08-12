use super::asset_request::{asset_release_ids, mcp_service_profile_acl, request_identity};
use crate::modules::assets::application::commands::BindMcpServiceProfile;
use crate::modules::assets::presentation::dto::McpServiceProfileResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;

pub fn mcp_service_profile_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ASSET_WRITE])?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/mcp-service-profile",
                move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (organization_id, asset_id, asset_release_id) =
                        asset_release_ids(&request)?;
                    let resource_access =
                        resource_access_evaluator(&request.require_auth_principal()?)?;
                    let acl = mcp_service_profile_acl(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(BindMcpServiceProfile {
                            organization_id,
                            asset_id,
                            asset_release_id,
                            resource_access,
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
        )?)
}
