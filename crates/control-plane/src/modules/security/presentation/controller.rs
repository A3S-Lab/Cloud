use super::dto::GatewayRoutePolicyTimelinePageResponse;
use crate::modules::security::{
    ListGatewayRoutePolicyTimeline, DEFAULT_SECURITY_TIMELINE_LIMIT,
    MAXIMUM_SECURITY_TIMELINE_LIMIT,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RouteId};
use crate::presentation::{
    application_error_response, organization_administrator_read_controller, request_id,
};
use a3s_boot::{
    controller, get, BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn security_investigation_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    organization_administrator_read_controller(
        Arc::new(SecurityInvestigationController { bus }).controller()?,
    )
}

#[derive(Debug, Clone)]
struct SecurityInvestigationController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
impl SecurityInvestigationController {
    #[get(
        "/{organization_id}/security-investigations/gateway-routes/{route_id}/timeline",
        raw
    )]
    async fn timeline(&self, request: BootRequest) -> Result<BootResponse> {
        let parameters: SecurityTimelineParameters = request.query()?;
        if parameters.limit == 0 || parameters.limit > MAXIMUM_SECURITY_TIMELINE_LIMIT {
            return Err(BootError::BadRequest(format!(
                "security timeline limit must be between 1 and {MAXIMUM_SECURITY_TIMELINE_LIMIT}"
            )));
        }
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListGatewayRoutePolicyTimeline {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                route_id: RouteId::from_uuid(request.param_as::<Uuid>("route_id")?),
                cursor: parameters.cursor,
                limit: parameters.limit,
            })
            .await?
        {
            Ok(page) => {
                BootResponse::json(&GatewayRoutePolicyTimelinePageResponse::from(page))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SecurityTimelineParameters {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

const fn default_limit() -> usize {
    DEFAULT_SECURITY_TIMELINE_LIMIT
}

#[cfg(test)]
mod nest_macro_security_investigation_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn security_investigation_controller_registers_scoped_route_via_nest_macros() {
        let controller = security_investigation_controller(Arc::new(QueryBus::new()))
            .expect("security nest query controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/security-investigations/gateway-routes/{route_id}/timeline"
        );
        // Scope metadata comes from organization_administrator_read_controller.
        assert_eq!(
            routes[0]
                .metadata()
                .get("auth.scopes")
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!(["cloud:read"])
        );
    }
}
