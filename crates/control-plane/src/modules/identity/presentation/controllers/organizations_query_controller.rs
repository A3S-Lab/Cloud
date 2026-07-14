use crate::modules::identity::application::queries::list_organizations::ListOrganizations;
use crate::modules::identity::presentation::dto::OrganizationListItemResponse;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn organizations_query_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?.get("/", move |request: BootRequest| {
        let bus = Arc::clone(&bus);
        async move {
            let principal = request.require_auth_principal()?;
            let organization_id = if principal.has_role("platform_admin") {
                None
            } else {
                let value = principal
                    .claim("organization_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        BootError::Forbidden(
                            "authenticated principal has no organization context".into(),
                        )
                    })?;
                Some(OrganizationId::from_uuid(Uuid::parse_str(value).map_err(
                    |error| {
                        BootError::Internal(format!(
                            "authenticated organization claim is invalid: {error}"
                        ))
                    },
                )?))
            };
            let request_id = request_id(&request)?;
            match bus.execute(ListOrganizations { organization_id }).await? {
                Ok(organizations) => BootResponse::json(
                    &organizations
                        .into_iter()
                        .map(OrganizationListItemResponse::from)
                        .collect::<Vec<_>>(),
                ),
                Err(error) => application_error_response(error, request_id),
            }
        }
    })
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
