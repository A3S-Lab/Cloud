use super::{CreateApiToken, CreateApiTokenResult};
use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::identity::domain::events::ApiTokenCreated;
use crate::modules::identity::domain::repositories::IApiTokenRepository;
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
            if !scopes.is_subset(&command.issuer_scopes) {
                return Ok(Err(ApplicationError::Forbidden(
                    "API token scopes cannot exceed the issuer's scopes".into(),
                )));
            }
            let digest = secret.digest();
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
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
            let result = match repository.create(token, digest, event, idempotency).await {
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
