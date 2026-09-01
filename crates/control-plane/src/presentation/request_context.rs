use crate::modules::identity::presentation::authenticated_actor;
use crate::modules::shared_kernel::domain::PrincipalId;
use a3s_boot::{BootError, BootRequest, Result};
use uuid::Uuid;

pub(crate) fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    Ok((idempotency_key, request_id(request)?))
}

pub(crate) fn actor_principal_id(request: &BootRequest) -> Result<PrincipalId> {
    let principal = request.require_auth_principal()?;
    Ok(authenticated_actor(&principal)?.principal_id)
}

pub(crate) fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
