use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::plugins::application::{
    ConfirmPluginPlanProjection, GetPluginPlanProjection,
};
use crate::modules::plugins::presentation::dto::{
    ConfirmPluginPlanProjectionRequest, PluginPlanProjectionResponse,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginPlanProjectionId};
use crate::presentation::{application_error_response, request_identity, request_id};
use a3s_boot::{
    controller, get, metadata, put, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn plugin_plan_projection_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(PluginPlanProjectionCommandsController { bus }).controller()
}

pub fn plugin_plan_projection_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(PluginPlanProjectionQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct PluginPlanProjectionCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct PluginPlanProjectionQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::PLUGIN_WRITE])]
impl PluginPlanProjectionCommandsController {
    #[put(
        "/{organization_id}/plugin-plan-projections/{projection_id}/confirmation",
        raw
    )]
    async fn confirm(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ConfirmPluginPlanProjectionRequest = request.json_with_content_type()?;
        let (_idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(ConfirmPluginPlanProjection {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                projection_id: PluginPlanProjectionId::from_uuid(
                    request.param_as::<Uuid>("projection_id")?,
                ),
                confirmation: body.confirmation,
                confirmed_at: Utc::now(),
            })
            .await?
        {
            Ok(projection) => {
                BootResponse::json(&PluginPlanProjectionResponse::from(projection))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl PluginPlanProjectionQueriesController {
    #[get("/{organization_id}/plugin-plan-projections/{projection_id}", raw)]
    async fn get_projection(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetPluginPlanProjection {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                projection_id: PluginPlanProjectionId::from_uuid(
                    request.param_as::<Uuid>("projection_id")?,
                ),
            })
            .await?
        {
            Ok(projection) => {
                BootResponse::json(&PluginPlanProjectionResponse::from(projection))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_plugin_plan_projection_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn plugin_plan_projection_queries_register_scoped_guarded_get_via_nest_macros() {
        let controller = plugin_plan_projection_queries_controller(Arc::new(QueryBus::new()))
            .expect("plugin plan projection nest query controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/plugin-plan-projections/{projection_id}"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::CLOUD_READ])
        );
    }

    #[test]
    fn plugin_plan_projection_commands_register_scoped_guarded_put_via_nest_macros() {
        let controller = plugin_plan_projection_commands_controller(Arc::new(CommandBus::new()))
            .expect("plugin plan projection nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Put);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/plugin-plan-projections/{projection_id}/confirmation"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::PLUGIN_WRITE])
        );
    }
}
