use super::asset_request::asset_release_ids;
use crate::modules::assets::application::queries::GetMcpServiceProfile;
use crate::modules::assets::presentation::dto::McpServiceProfileResponse;
use crate::presentation::{
    application_error_response, asset_access, organization_tenant_cloud_read_controller,
    request_id, resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
};
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, RouteDefinition,
};
use std::sync::Arc;

pub fn mcp_service_profile_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new("/organizations")?
        .route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/mcp-service-profile",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (organization_id, asset_id, asset_release_id) =
                        asset_release_ids(&request)?;
                    let access = asset_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetMcpServiceProfile {
                            organization_id,
                            asset_id,
                            asset_release_id,
                            access,
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
        )?,
        DeferredResourceScope::Any,
    )?)?;
    organization_tenant_cloud_read_controller(controller)
}
