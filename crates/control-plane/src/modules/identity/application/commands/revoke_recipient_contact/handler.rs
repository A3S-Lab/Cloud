use super::RevokeRecipientContact;
use crate::modules::identity::application::RecipientContactMutationResult;
use crate::modules::identity::domain::repositories::{
    IRecipientContactRepository, RevokeRecipientContactWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct RevokeRecipientContactHandler {
    repository: Arc<dyn IRecipientContactRepository>,
}

impl RevokeRecipientContactHandler {
    pub fn new(repository: Arc<dyn IRecipientContactRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<RevokeRecipientContact> for RevokeRecipientContactHandler {
    fn execute(
        &self,
        command: RevokeRecipientContact,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<RecipientContactMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected recipient contact version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "principalId": command.actor_principal_id,
                "contactId": command.contact_id,
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/principals/{}/recipient-contacts/{}/revocation",
                    command.organization_id, command.actor_principal_id, command.contact_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .revoke_recipient_contact(RevokeRecipientContactWrite {
                    organization_id: command.organization_id,
                    actor_principal_id: command.actor_principal_id,
                    contact_id: command.contact_id,
                    expected_version: command.expected_version,
                    revoked_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(RecipientContactMutationResult {
                contact: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
