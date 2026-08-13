use super::AcceptMembershipInvitation;
use crate::modules::identity::application::MembershipInvitationAcceptanceResult;
use crate::modules::identity::domain::repositories::{
    AcceptMembershipInvitationWrite, IMembershipInvitationRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, MembershipId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct AcceptMembershipInvitationHandler {
    repository: Arc<dyn IMembershipInvitationRepository>,
}

impl AcceptMembershipInvitationHandler {
    pub fn new(repository: Arc<dyn IMembershipInvitationRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<AcceptMembershipInvitation> for AcceptMembershipInvitationHandler {
    fn execute(
        &self,
        command: AcceptMembershipInvitation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<MembershipInvitationAcceptanceResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected membership invitation version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "invitationId": command.invitation_id,
                "expectedVersion": command.expected_version,
                "actorPrincipalId": command.actor_principal_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "membership-invitations/{}/acceptance",
                    command.invitation_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .accept_membership_invitation(AcceptMembershipInvitationWrite {
                    invitation_id: command.invitation_id,
                    expected_version: command.expected_version,
                    membership_id: MembershipId::new(),
                    actor_principal_id: command.actor_principal_id,
                    accepted_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(MembershipInvitationAcceptanceResult {
                acceptance: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
