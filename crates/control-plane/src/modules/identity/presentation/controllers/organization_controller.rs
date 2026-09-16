use crate::modules::identity::application::commands::create_organization::CreateOrganization;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    CreateOrganizationRequest, OrganizationResponse,
};
use crate::modules::identity::presentation::request_context::{actor, mutation_identity};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, metadata, post, AUTH_SCOPES_METADATA, BootRequest, BootResponse, CommandBus,
    ControllerDefinition, Result,
};
use std::sync::Arc;

pub fn organization_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(OrganizationController { bus }).controller()
}

#[derive(Debug, Clone)]
struct OrganizationController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[metadata("auth.scopes", vec![ApiTokenScope::PLATFORM_WRITE])]
impl OrganizationController {
    #[post("/", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateOrganizationRequest = request.json_with_content_type()?;
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
            .execute(CreateOrganization {
                name: body.name,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &OrganizationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_organization_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn organization_controller_registers_scoped_post_via_nest_macros() {
        let controller = organization_controller(Arc::new(CommandBus::new()))
            .expect("organization nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(routes[0].path(), "/organizations");
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::PLATFORM_WRITE])
        );
    }
}
