use super::tool_result;
use crate::modules::security::presentation::GatewayRoutePolicyTimelinePageResponse;
use crate::modules::security::ListGatewayRoutePolicyTimeline;
use crate::modules::shared_kernel::domain::{OrganizationId, RouteId};
use a3s_boot::{QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecurityGatewayRoutePolicyTimelineArguments {
    route_id: Uuid,
    cursor: Option<String>,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_security_timeline_limit"
    )]
    limit: usize,
}

pub async fn list_gateway_route_policy_timeline(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: SecurityGatewayRoutePolicyTimelineArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListGatewayRoutePolicyTimeline {
            organization_id,
            route_id: RouteId::from_uuid(arguments.route_id),
            cursor: arguments.cursor,
            limit: arguments.limit,
        })
        .await?
    {
        Ok(page) => tool_result::success(
            200,
            GatewayRoutePolicyTimelinePageResponse::from(page),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
