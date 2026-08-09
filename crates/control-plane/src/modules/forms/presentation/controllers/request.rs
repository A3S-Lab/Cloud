use crate::modules::shared_kernel::domain::PrincipalId;
use a3s_boot::{BootError, BootRequest, Result};
use uuid::Uuid;

pub(super) fn actor_principal_id(request: &BootRequest) -> Result<PrincipalId> {
    let principal = request.require_auth_principal()?;
    Uuid::parse_str(principal.subject())
        .map(PrincipalId::from_uuid)
        .map_err(|error| {
            BootError::Internal(format!(
                "authenticated principal identity is invalid: {error}"
            ))
        })
}

pub(super) fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
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

pub(super) fn expected_version(request: &BootRequest) -> Result<u64> {
    let expected_version = request
        .header("x-a3s-expected-version")
        .ok_or_else(|| BootError::BadRequest("x-a3s-expected-version header is required".into()))?
        .parse::<u64>()
        .map_err(|_| {
            BootError::BadRequest("x-a3s-expected-version must be a positive integer".into())
        })?;
    if expected_version == 0 {
        return Err(BootError::BadRequest(
            "x-a3s-expected-version must be a positive integer".into(),
        ));
    }
    Ok(expected_version)
}
