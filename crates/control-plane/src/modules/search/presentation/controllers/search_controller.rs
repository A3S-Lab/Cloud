use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::search::application::SearchResources;
use crate::modules::search::presentation::dto::SearchResultResponse;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn search_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get("/{organization_id}/search", move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let parameters: SearchParameters = request.query()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(SearchResources {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        query: parameters.query.unwrap_or_default(),
                        limit: parameters.limit,
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
        })
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

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
