use super::RevokeMembershipInvitation;
use crate::modules::identity::application::MembershipInvitationMutationResult;
use crate::modules::identity::domain::repositories::{
    IMembershipInvitationRepository, RevokeMembershipInvitationWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct RevokeMembershipInvitationHandler {
    repository: Arc<dyn IMembershipInvitationRepository>,
}

impl RevokeMembershipInvitationHandler {
    pub fn new(repository: Arc<dyn IMembershipInvitationRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<RevokeMembershipInvitation> for RevokeMembershipInvitationHandler {
    fn execute(
        &self,
        command: RevokeMembershipInvitation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<MembershipInvitationMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected membership invitation version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "invitationId": command.invitation_id,
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/membership-invitations/{}/revocation",
                    command.organization_id, command.invitation_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .revoke_membership_invitation(RevokeMembershipInvitationWrite {
                    organization_id: command.organization_id,
                    invitation_id: command.invitation_id,
                    expected_version: command.expected_version,
                    actor_principal_id: command.actor_principal_id,
                    actor_is_platform_admin: command.actor_is_platform_admin,
                    revoked_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(MembershipInvitationMutationResult {
                invitation: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
