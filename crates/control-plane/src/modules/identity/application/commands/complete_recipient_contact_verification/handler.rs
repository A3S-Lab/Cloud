use super::CompleteRecipientContactVerification;
use crate::modules::identity::application::commands::map_recipient_contact_proof_error;
use crate::modules::identity::application::RecipientContactMutationResult;
use crate::modules::identity::domain::repositories::{
    CompleteRecipientContactVerificationWrite, IRecipientContactRepository,
};
use crate::modules::identity::domain::services::IRecipientContactProofService;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CompleteRecipientContactVerificationHandler {
    repository: Arc<dyn IRecipientContactRepository>,
    proof_service: Arc<dyn IRecipientContactProofService>,
}

impl CompleteRecipientContactVerificationHandler {
    pub fn new(
        repository: Arc<dyn IRecipientContactRepository>,
        proof_service: Arc<dyn IRecipientContactProofService>,
    ) -> Self {
        Self {
            repository,
            proof_service,
        }
    }
}

impl CommandHandler<CompleteRecipientContactVerification>
    for CompleteRecipientContactVerificationHandler
{
    fn execute(
        &self,
        command: CompleteRecipientContactVerification,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<RecipientContactMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        let proof_service = Arc::clone(&self.proof_service);
        Box::pin(async move {
            let completed_at = Utc::now();
            let claims = match proof_service.verify(&command.proof, completed_at).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(map_recipient_contact_proof_error(error))),
            };
            if claims.contact_id != command.contact_id
                || claims.principal_id != command.actor_principal_id
            {
                return Ok(Err(ApplicationError::Forbidden(
                    "recipient contact verification proof was rejected".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "principalId": command.actor_principal_id,
                "contactId": command.contact_id,
                "claims": claims,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/principals/{}/recipient-contacts/{}/verification",
                    command.organization_id, command.actor_principal_id, command.contact_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .complete_recipient_contact_verification(
                    CompleteRecipientContactVerificationWrite {
                        organization_id: command.organization_id,
                        actor_principal_id: command.actor_principal_id,
                        contact_id: command.contact_id,
                        claims,
                        completed_at,
                        request_id: command.request_id,
                        idempotency,
                    },
                )
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
