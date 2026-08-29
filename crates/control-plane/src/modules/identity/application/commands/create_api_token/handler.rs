use super::{CreateApiToken, CreateApiTokenResult};
use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::identity::domain::events::ApiTokenCreated;
use crate::modules::identity::domain::repositories::{CreateApiTokenWrite, IApiTokenRepository};
use crate::modules::identity::domain::value_objects::{
    ApiTokenName, ApiTokenScope, ApiTokenSecret,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{ApiTokenId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::collections::BTreeSet;
use std::sync::Arc;

pub struct CreateApiTokenHandler {
    repository: Arc<dyn IApiTokenRepository>,
}

impl CreateApiTokenHandler {
    pub fn new(repository: Arc<dyn IApiTokenRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<CreateApiToken> for CreateApiTokenHandler {
    fn execute(
        &self,
        command: CreateApiToken,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CreateApiTokenResult>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let name = match ApiTokenName::parse(command.name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let secret = match ApiTokenSecret::parse(command.token_secret) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let scopes = match command
                .scopes
                .into_iter()
                .map(ApiTokenScope::parse)
                .collect::<Result<BTreeSet<_>, _>>()
            {
                Ok(value) if !value.is_empty() => value,
                Ok(_) => {
                    return Ok(Err(ApplicationError::Invalid(
                        "API token must grant at least one scope".into(),
                    )))
                }
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            // Every authenticated organization token already has baseline read access.
            // This explicit scope lets a token manager delegate that access without a write scope.
            let mut issuer_scopes = command.issuer_scopes;
            issuer_scopes.insert(
                ApiTokenScope::parse(ApiTokenScope::CLOUD_READ).map_err(BootError::Internal)?,
            );
            if !scopes.is_subset(&issuer_scopes) {
                return Ok(Err(ApplicationError::Forbidden(
                    "API token scopes cannot exceed the issuer's scopes".into(),
                )));
            }
            let digest = secret.digest();
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "principalId": command.principal_id,
                "name": name.as_str(),
                "tokenDigest": digest.as_str(),
                "scopes": scopes,
                "expiresAt": command.expires_at,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!("organizations/{}/api-tokens", command.organization_id),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let token = match ApiToken::issue(
                ApiTokenId::new(),
                command.organization_id,
                command.principal_id,
                name,
                scopes,
                Utc::now(),
                command.expires_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = ApiTokenCreated::envelope(&token, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match repository
                .create(CreateApiTokenWrite {
                    token,
                    digest,
                    event,
                    issuer_principal_id: command.issuer_principal_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateApiTokenResult {
                api_token: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
