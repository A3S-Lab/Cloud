use crate::modules::shared_kernel::domain::{ApiTokenId, PrincipalId};
use a3s_boot::{AuthPrincipal, BootError, BootRequest, Result};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(crate) struct IdentityActor {
    pub principal_id: PrincipalId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AuthenticatedCredentialActor {
    pub principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
}

pub(super) fn actor(request: &BootRequest) -> Result<IdentityActor> {
    let principal = request.require_auth_principal()?;
    authenticated_actor(&principal)
}

pub(crate) fn authenticated_actor(principal: &AuthPrincipal) -> Result<IdentityActor> {
    let principal_id = Uuid::parse_str(principal.subject()).map_err(|error| {
        BootError::Internal(format!(
            "authenticated principal identity is invalid: {error}"
        ))
    })?;
    Ok(IdentityActor {
        principal_id: PrincipalId::from_uuid(principal_id),
    })
}

pub(crate) fn authenticated_credential_actor(
    principal: &AuthPrincipal,
) -> Result<AuthenticatedCredentialActor> {
    let actor = authenticated_actor(principal)?;
    let credential_id = principal
        .claim("credential_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| BootError::Internal("authenticated credential identity is missing".into()))?
        .parse::<Uuid>()
        .map(ApiTokenId::from_uuid)
        .map_err(|error| {
            BootError::Internal(format!(
                "authenticated credential identity is invalid: {error}"
            ))
        })?;
    Ok(AuthenticatedCredentialActor {
        principal_id: actor.principal_id,
        credential_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_credential_actor_uses_server_verified_claims() {
        let principal_id = PrincipalId::new();
        let credential_id = ApiTokenId::new();
        let principal = AuthPrincipal::new(principal_id.to_string())
            .with_role("platform_admin")
            .with_claim("credential_id", credential_id.to_string())
            .expect("credential claim");

        let actor = authenticated_credential_actor(&principal).expect("actor");

        assert_eq!(actor.principal_id, principal_id);
        assert_eq!(actor.credential_id, credential_id);
    }

    #[test]
    fn authenticated_credential_actor_requires_credential_claim() {
        let principal = AuthPrincipal::new(PrincipalId::new().to_string());

        assert!(authenticated_credential_actor(&principal).is_err());
    }
}
