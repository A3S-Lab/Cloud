use super::asset_request::{asset_release_ids, request_id};
use crate::modules::assets::application::queries::GetMcpServiceProfile;
use crate::modules::assets::presentation::dto::McpServiceProfileResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, AUTH_SCOPES_METADATA,
};
use std::sync::Arc;

pub fn mcp_service_profile_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/mcp-service-profile",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (organization_id, asset_id, asset_release_id) =
                        asset_release_ids(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetMcpServiceProfile {
                            organization_id,
                            asset_id,
                            asset_release_id,
                        })
                        .await?
                    {
                        Ok(binding) => {
                            BootResponse::json(&McpServiceProfileResponse::from(binding))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
