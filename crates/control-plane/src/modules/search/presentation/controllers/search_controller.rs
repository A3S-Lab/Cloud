use crate::modules::search::application::SearchResources;
use crate::modules::search::presentation::dto::SearchResultResponse;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::{
    application_error_response, request_id, resource_access_evaluator, search_visibility,
    OrganizationTenantGuard,
};
use a3s_boot::{
    controller, get, use_guard, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn search_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(SearchController { bus }).controller()
}

#[derive(Debug, Clone)]
struct SearchController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl SearchController {
    #[get("/{organization_id}/search", raw)]
    async fn search(&self, request: BootRequest) -> Result<BootResponse> {
        let parameters: SearchParameters = request.query()?;
        let request_id = request_id(&request)?;
        let resource_access = resource_access_evaluator(&request.require_auth_principal()?)?;
        let visibility = search_visibility(&resource_access);
        match self
            .bus
            .execute(SearchResources {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                query: parameters.query.unwrap_or_default(),
                limit: parameters.limit,
                visibility,
            })
            .await?
        {
            Ok(results) => BootResponse::json(
                &results
                    .into_iter()
                    .map(SearchResultResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchParameters {
    #[serde(default, rename = "q")]
    query: Option<String>,
    #[serde(default = "default_limit")]
    limit: u16,
}

const fn default_limit() -> u16 {
    20
}

#[cfg(test)]
mod nest_macro_search_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn search_controller_registers_guarded_get_via_nest_macros() {
        let controller =
            search_controller(Arc::new(QueryBus::new())).expect("search nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(routes[0].path(), "/organizations/{organization_id}/search");
    }
}
