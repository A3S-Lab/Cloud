use crate::modules::identity::domain::repositories::IApiTokenRepository;
use crate::modules::identity::domain::value_objects::{ApiTokenScope, ApiTokenSecret};
use a3s_boot::{
    AuthPrincipal, BearerTokenVerifier, BootError, BoxFuture, ExecutionContext, Result,
};
use chrono::Utc;
use std::sync::Arc;

#[derive(Clone)]
pub struct ApiTokenVerifier {
    repository: Arc<dyn IApiTokenRepository>,
}

impl ApiTokenVerifier {
    pub fn new(repository: Arc<dyn IApiTokenRepository>) -> Self {
        Self { repository }
    }
}

impl BearerTokenVerifier for ApiTokenVerifier {
    fn verify(
        &self,
        token: String,
        _context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<AuthPrincipal>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let Ok(secret) = ApiTokenSecret::parse(token) else {
                return Ok(None);
            };
            let authenticated = repository
                .authenticate(&secret.digest(), Utc::now())
                .await
                .map_err(|error| {
                    BootError::Internal(format!("API token verification failed: {error}"))
                })?;
            let Some(token) = authenticated else {
                return Ok(None);
            };
            let mut principal = AuthPrincipal::new(token.id.to_string())
                .with_claim("organization_id", token.organization_id.to_string())?
                .with_scopes(token.scopes.iter().map(ApiTokenScope::as_str));
            if token
                .scopes
                .iter()
                .any(|scope| scope.as_str() == ApiTokenScope::PLATFORM_WRITE)
            {
                principal = principal.with_role("platform_admin");
            }
            Ok(Some(principal))
        })
    }
}
