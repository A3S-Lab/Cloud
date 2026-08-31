use crate::modules::search::application::SearchResources;
use crate::modules::search::presentation::dto::SearchResultResponse;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::{
    application_error_response, request_id, resource_access_evaluator, search_visibility,
    OrganizationTenantGuard,
};
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
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
                let resource_access =
                    resource_access_evaluator(&request.require_auth_principal()?)?;
                let visibility = search_visibility(&resource_access);
                match bus
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
