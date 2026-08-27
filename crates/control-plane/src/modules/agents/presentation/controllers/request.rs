use crate::modules::identity::presentation::{
    authenticated_credential_actor, AuthenticatedCredentialActor,
};
pub(super) use crate::presentation::{request_id, request_identity};
use a3s_boot::{BootError, BootRequest, Result};

pub(super) fn credential_actor(request: &BootRequest) -> Result<AuthenticatedCredentialActor> {
    authenticated_credential_actor(&request.require_auth_principal()?)
}

pub(super) fn expected_version(request: &BootRequest) -> Result<u64> {
    let value = request
        .header("x-a3s-expected-version")
        .ok_or_else(|| BootError::BadRequest("x-a3s-expected-version header is required".into()))?
        .parse::<u64>()
        .map_err(|_| {
            BootError::BadRequest("x-a3s-expected-version must be a positive integer".into())
        })?;
    if value == 0 {
        return Err(BootError::BadRequest(
            "x-a3s-expected-version must be a positive integer".into(),
        ));
    }
    Ok(value)
}
