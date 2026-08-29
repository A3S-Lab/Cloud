use crate::modules::identity::application::RESOURCE_GRANT_SCOPES_CLAIM;
use crate::modules::identity::domain::repositories::{
    IApiTokenRepository, IResourceGrantRepository,
};
use crate::modules::identity::domain::value_objects::{
    ApiTokenScope, ApiTokenSecret, MembershipRole,
};
use a3s_boot::{
    AuthPrincipal, BearerTokenVerifier, BootError, BoxFuture, ExecutionContext, Result,
};
use chrono::Utc;
use std::sync::Arc;

#[derive(Clone)]
pub struct ApiTokenVerifier {
    api_tokens: Arc<dyn IApiTokenRepository>,
    resource_grants: Arc<dyn IResourceGrantRepository>,
}

impl ApiTokenVerifier {
    pub fn new(
        api_tokens: Arc<dyn IApiTokenRepository>,
        resource_grants: Arc<dyn IResourceGrantRepository>,
    ) -> Self {
        Self {
            api_tokens,
            resource_grants,
        }
    }
}

impl BearerTokenVerifier for ApiTokenVerifier {
    fn verify(
        &self,
        token: String,
        _context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<AuthPrincipal>>> {
        let api_tokens = Arc::clone(&self.api_tokens);
        let resource_grants = Arc::clone(&self.resource_grants);
        Box::pin(async move {
            let Ok(secret) = ApiTokenSecret::parse(token) else {
                return Ok(None);
            };
            let authenticated = api_tokens
                .authenticate(&secret.digest(), Utc::now())
                .await
                .map_err(|error| {
                    BootError::Internal(format!("API token verification failed: {error}"))
                })?;
            let Some(authenticated) = authenticated else {
                return Ok(None);
            };
            let token = authenticated.api_token;
            let mut principal = AuthPrincipal::new(authenticated.principal.id.to_string())
                .with_claim("credential_id", token.id.to_string())?
                .with_claim("organization_id", token.organization_id.to_string())?
                .with_scopes(token.scopes.iter().map(ApiTokenScope::as_str));
            if let Some(membership) = authenticated.membership {
                let granted_scopes = if membership.role == MembershipRole::Restricted {
                    resource_grants
                        .list_active_resource_grants_for_membership(
                            membership.organization_id,
                            membership.id,
                        )
                        .await
                        .map_err(|error| {
                            BootError::Internal(format!(
                                "Resource Grant verification failed: {error}"
                            ))
                        })?
                        .into_iter()
                        .map(|grant| grant.scope)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                principal = principal
                    .with_claim("membership_id", membership.id.to_string())?
                    .with_claim("organization_role", membership.role.as_str())?
                    .with_claim(RESOURCE_GRANT_SCOPES_CLAIM, granted_scopes)?
                    .with_role(format!("organization_{}", membership.role.as_str()));
            }
            Ok(Some(principal))
        })
    }
}
