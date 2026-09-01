use crate::modules::forms::FormAccess;
pub(super) use crate::presentation::{actor_principal_id, request_id, request_identity};
use crate::presentation::{form_access as project_form_access, resource_access_evaluator};
use a3s_boot::{BootError, BootRequest, Result};

/// Projects authenticated Identity authority once into Forms vocabulary at
/// the inbound adapter boundary.
pub(super) fn form_access(request: &BootRequest) -> Result<FormAccess> {
    Ok(project_form_access(&resource_access_evaluator(
        &request.require_auth_principal()?,
    )?))
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
