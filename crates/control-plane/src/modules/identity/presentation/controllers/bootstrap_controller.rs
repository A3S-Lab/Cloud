use crate::modules::identity::application::commands::bootstrap_identity::BootstrapIdentity;
use crate::modules::identity::presentation::dto::{
    BootstrapIdentityRequest, BootstrapIdentityResponse,
};
use crate::modules::identity::presentation::BootstrapGuard;
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_PUBLIC_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn bootstrap_controller(
    bus: Arc<CommandBus>,
    guard: BootstrapGuard,
) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/bootstrap")?
        .with_guard(guard)
        .with_metadata(AUTH_PUBLIC_METADATA, true)?
        .post("/", move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: BootstrapIdentityRequest = request.json_with_content_type()?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(BootstrapIdentity {
                        organization_name: body.organization_name,
                        token_name: body.token_name,
                        token_secret: body.token,
                        expires_at: body.expires_at,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 201 };
                        BootResponse::json_with_status(
                            status,
                            &BootstrapIdentityResponse::from(result),
                        )
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })
}

fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    let request_id = request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })?;
    Ok((idempotency_key, request_id))
}
