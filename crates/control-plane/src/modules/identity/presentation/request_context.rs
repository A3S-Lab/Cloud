use crate::modules::shared_kernel::domain::PrincipalId;
use a3s_boot::{BootError, BootRequest, Result};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(super) struct IdentityActor {
    pub principal_id: PrincipalId,
    pub is_platform_admin: bool,
}

pub(super) fn actor(request: &BootRequest) -> Result<IdentityActor> {
    let principal = request.require_auth_principal()?;
    let principal_id = Uuid::parse_str(principal.subject()).map_err(|error| {
        BootError::Internal(format!(
            "authenticated principal identity is invalid: {error}"
        ))
    })?;
    Ok(IdentityActor {
        principal_id: PrincipalId::from_uuid(principal_id),
        is_platform_admin: principal.has_role("platform_admin"),
    })
}

pub(super) fn mutation_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    Ok((idempotency_key, request_id(request)?))
}

pub(super) fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
