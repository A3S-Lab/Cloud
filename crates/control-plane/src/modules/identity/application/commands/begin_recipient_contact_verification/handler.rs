use super::BeginRecipientContactVerification;
use crate::modules::identity::application::RecipientContactVerificationRequestResult;
use crate::modules::identity::domain::repositories::{
    BeginRecipientContactVerificationWrite, IRecipientContactRepository,
};
use crate::modules::identity::domain::services::IRecipientContactProofService;
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, RecipientContactId, RecipientContactVerificationId,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::{Duration, Utc};
use std::sync::Arc;

const RECIPIENT_CONTACT_VERIFICATION_LIFETIME: Duration = Duration::minutes(10);

pub struct BeginRecipientContactVerificationHandler {
    repository: Arc<dyn IRecipientContactRepository>,
    proof_service: Arc<dyn IRecipientContactProofService>,
}

impl BeginRecipientContactVerificationHandler {
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

impl CommandHandler<BeginRecipientContactVerification>
    for BeginRecipientContactVerificationHandler
{
    fn execute(
        &self,
        command: BeginRecipientContactVerification,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<RecipientContactVerificationRequestResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        let proof_service = Arc::clone(&self.proof_service);
        Box::pin(async move {
            let address = match RecipientEmailAddress::parse(command.address.as_str()) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "principalId": command.actor_principal_id,
                "address": address.as_str(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/principals/{}/recipient-contacts/verifications",
                    command.organization_id, command.actor_principal_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let requested_at = Utc::now();
            let result = match repository
                .begin_recipient_contact_verification(BeginRecipientContactVerificationWrite {
                    organization_id: command.organization_id,
                    actor_principal_id: command.actor_principal_id,
                    contact_id: RecipientContactId::new(),
                    verification_id: RecipientContactVerificationId::new(),
                    address,
                    signing_key_id: proof_service.current_key_id().clone(),
                    requested_at,
                    expires_at: requested_at + RECIPIENT_CONTACT_VERIFICATION_LIFETIME,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(RecipientContactVerificationRequestResult {
                contact: result.value.contact,
                verification: result.value.verification,
                replayed: result.replayed,
            }))
        })
    }
}
