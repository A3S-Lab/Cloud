use crate::modules::identity::application::commands::create_organization::CreateOrganization;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    CreateOrganizationRequest, OrganizationResponse,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn organization_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::PLATFORM_WRITE])?
        .post("/", move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: CreateOrganizationRequest = request.json_with_content_type()?;
                let idempotency_key = request
                    .header("idempotency-key")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        BootError::BadRequest("idempotency-key header is required".into())
                    })?
                    .to_owned();
                let request_id = request
                    .header("x-request-id")
                    .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
                    .and_then(|value| {
                        Uuid::parse_str(value).map_err(|error| {
                            BootError::Internal(format!("invalid request ID: {error}"))
                        })
                    })?;
                match bus
                    .execute(CreateOrganization {
                        name: body.name,
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
        })
}
