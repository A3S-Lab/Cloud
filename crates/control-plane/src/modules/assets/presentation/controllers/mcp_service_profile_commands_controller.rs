use super::asset_request::{asset_release_ids, mcp_service_profile_acl, request_identity};
use crate::modules::assets::application::commands::BindMcpServiceProfile;
use crate::modules::assets::presentation::dto::McpServiceProfileResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, AUTH_SCOPES_METADATA,
};
use std::sync::Arc;

pub fn mcp_service_profile_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ASSET_WRITE])?
        .post(
            "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/mcp-service-profile",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (organization_id, asset_id, asset_release_id) =
                        asset_release_ids(&request)?;
                    let acl = mcp_service_profile_acl(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(BindMcpServiceProfile {
                            organization_id,
                            asset_id,
                            asset_release_id,
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
        )
}
