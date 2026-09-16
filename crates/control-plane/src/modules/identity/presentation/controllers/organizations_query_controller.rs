use crate::modules::identity::application::queries::list_organizations::ListOrganizations;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::OrganizationListItemResponse;
use crate::modules::identity::presentation::request_context::{
    authenticated_credential_actor, request_id,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;

pub fn organizations_query_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(OrganizationsQueryController { bus }).controller()
}

#[derive(Debug, Clone)]
struct OrganizationsQueryController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl OrganizationsQueryController {
    #[get("/", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListOrganizations {
                actor_principal_id: actor.principal_id,
                credential_id: actor.credential_id,
                request_id,
            })
            .await?
        {
            Ok(organizations) => BootResponse::json(
                &organizations
                    .into_iter()
                    .map(OrganizationListItemResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_organizations_query_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn organizations_query_controller_registers_scoped_get_root_via_nest_macros() {
        let controller = organizations_query_controller(Arc::new(QueryBus::new()))
            .expect("organizations nest query controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(routes[0].path(), "/organizations");
        let scopes = routes[0]
            .metadata()
            .get(AUTH_SCOPES_METADATA)
            .cloned()
            .expect("auth.scopes metadata");
        assert_eq!(
            scopes,
            serde_json::json!([ApiTokenScope::CLOUD_READ])
        );
    }
}
