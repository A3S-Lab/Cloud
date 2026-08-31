use super::tool_result;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::search::{SearchResources, SearchResultResponse};
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::search_visibility;
use a3s_boot::{QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchArguments {
    query: String,
    #[serde(default = "default_limit")]
    limit: u16,
}

pub async fn search(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: SearchArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    let visibility = search_visibility(&resource_access);
    match bus
        .execute(SearchResources {
            organization_id,
            query: arguments.query,
            limit: arguments.limit,
            visibility,
        })
        .await?
    {
        Ok(results) => tool_result::success(
            200,
            results
                .into_iter()
                .map(SearchResultResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

const fn default_limit() -> u16 {
    20
}
