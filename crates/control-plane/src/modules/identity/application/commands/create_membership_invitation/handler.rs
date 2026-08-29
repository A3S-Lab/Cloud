use super::CreateMembershipInvitation;
use crate::modules::identity::application::MembershipInvitationMutationResult;
use crate::modules::identity::domain::entities::MembershipInvitation;
use crate::modules::identity::domain::events::MembershipInvitationChanged;
use crate::modules::identity::domain::repositories::{
    CreateMembershipInvitationWrite, IMembershipInvitationRepository,
};
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, MembershipInvitationId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateMembershipInvitationHandler {
    repository: Arc<dyn IMembershipInvitationRepository>,
}

impl CreateMembershipInvitationHandler {
    pub fn new(repository: Arc<dyn IMembershipInvitationRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<CreateMembershipInvitation> for CreateMembershipInvitationHandler {
    fn execute(
        &self,
        command: CreateMembershipInvitation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<MembershipInvitationMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let role = match MembershipRole::parse(&command.role) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "principalId": command.principal_id,
                "role": role.as_str(),
                "expiresAt": command.expires_at,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/membership-invitations",
                    command.organization_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let invitation = match MembershipInvitation::create(
                MembershipInvitationId::new(),
                command.organization_id,
                command.principal_id,
                role,
                command.actor_principal_id,
                Utc::now(),
                command.expires_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = MembershipInvitationChanged::created(&invitation, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match repository
                .create_membership_invitation(CreateMembershipInvitationWrite {
                    invitation,
                    event,
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
