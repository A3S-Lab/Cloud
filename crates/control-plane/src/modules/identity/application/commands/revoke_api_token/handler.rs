use super::{RevokeApiToken, RevokeApiTokenResult};
use crate::modules::identity::domain::events::ApiTokenRevoked;
use crate::modules::identity::domain::repositories::IApiTokenRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct RevokeApiTokenHandler {
    repository: Arc<dyn IApiTokenRepository>,
}

impl RevokeApiTokenHandler {
    pub fn new(repository: Arc<dyn IApiTokenRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<RevokeApiToken> for RevokeApiTokenHandler {
    fn execute(
        &self,
        command: RevokeApiToken,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<RevokeApiTokenResult>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "tokenId": command.token_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/api-tokens/{}/revoke",
                    command.organization_id, command.token_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let mut token = match repository
                .find(command.organization_id, command.token_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "API token not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let event = if token.revoke(Utc::now()) {
                Some(
                    ApiTokenRevoked::envelope(&token, command.request_id)
                        .map_err(|error| BootError::Internal(error.to_string()))?,
                )
            } else {
                None
            };
            let result = match repository.revoke(token, event, idempotency).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(RevokeApiTokenResult {
                api_token: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
