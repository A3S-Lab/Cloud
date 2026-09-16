use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::plugins::application::EnrollPluginRegistry;
use crate::modules::plugins::presentation::dto::{
    EnrollPluginRegistryRequest, PluginRegistryMutationResponse, PluginRegistryResponse,
};
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::{actor_principal_id, application_error_response, request_identity};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, Result,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn plugin_registry_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(PluginRegistryCommandsController { bus }).controller()
}

#[derive(Debug, Clone)]
struct PluginRegistryCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::PLUGIN_WRITE])]
impl PluginRegistryCommandsController {
    #[post("/{organization_id}/plugin-registries", raw)]
    async fn enroll(&self, request: BootRequest) -> Result<BootResponse> {
        let body: EnrollPluginRegistryRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let actor_id = actor_principal_id(&request)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let bootstrap_root = STANDARD
            .decode(body.bootstrap_root_base64.trim())
            .map_err(|_| {
                BootError::BadRequest(
                    "bootstrapRootBase64 must be standard base64 encoding of the TUF bootstrap root"
                        .into(),
                )
            })?;
        match self
            .bus
            .execute(EnrollPluginRegistry {
                organization_id,
                actor_id,
                name: body.name,
                endpoint: body.endpoint,
                bootstrap_root,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &PluginRegistryMutationResponse {
                        registry: PluginRegistryResponse::from(result.registry),
                        replayed: result.replayed,
                    },
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_plugin_registry_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::BTreeSet;

    #[test]
    fn plugin_registry_commands_register_scoped_guarded_post_via_nest_macros() {
        let controller = plugin_registry_commands_controller(Arc::new(CommandBus::new()))
            .expect("plugin registry nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        let paths: BTreeSet<_> = routes
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect();
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/plugin-registries".into()
        )));
        for route in routes {
            assert_eq!(
                route
                    .metadata()
                    .get(AUTH_SCOPES_METADATA)
                    .cloned()
                    .expect("auth.scopes"),
                serde_json::json!([ApiTokenScope::PLUGIN_WRITE])
            );
        }
    }
}
