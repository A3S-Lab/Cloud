use super::dto::GatewayRoutePolicyTimelinePageResponse;
use crate::modules::security::{
    ListGatewayRoutePolicyTimeline, DEFAULT_SECURITY_TIMELINE_LIMIT,
    MAXIMUM_SECURITY_TIMELINE_LIMIT,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RouteId};
use crate::presentation::{
    application_error_response, organization_administrator_read_controller, request_id,
};
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn security_investigation_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    organization_administrator_read_controller(ControllerDefinition::new("/organizations")?)?
        .get(
            "/{organization_id}/security-investigations/gateway-routes/{route_id}/timeline",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let parameters: SecurityTimelineParameters = request.query()?;
                    if parameters.limit == 0 || parameters.limit > MAXIMUM_SECURITY_TIMELINE_LIMIT {
                        return Err(BootError::BadRequest(format!(
                            "security timeline limit must be between 1 and {MAXIMUM_SECURITY_TIMELINE_LIMIT}"
                        )));
                    }
                    let request_id = request_id(&request)?;
                    match bus
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
                        Ok(page) => BootResponse::json(
                            &GatewayRoutePolicyTimelinePageResponse::from(page),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
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
