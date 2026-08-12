use super::asset_request::{asset_release_ids, request_id};
use crate::modules::assets::application::queries::GetMcpServiceProfile;
use crate::modules::assets::presentation::dto::McpServiceProfileResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, RouteDefinition,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;

pub fn mcp_service_profile_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/mcp-service-profile",
                move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (organization_id, asset_id, asset_release_id) =
                        asset_release_ids(&request)?;
                    let resource_access =
                        resource_access_evaluator(&request.require_auth_principal()?)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetMcpServiceProfile {
                            organization_id,
                            asset_id,
                            asset_release_id,
                            resource_access,
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
        )?)
}
