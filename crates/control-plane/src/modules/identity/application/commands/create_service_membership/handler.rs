use super::CreateServiceMembership;
use crate::modules::identity::application::MembershipMutationResult;
use crate::modules::identity::domain::entities::{IdentityPrincipal, Membership};
use crate::modules::identity::domain::events::{MembershipChanged, PrincipalCreated};
use crate::modules::identity::domain::repositories::{
    CreateServiceMembershipWrite, IMembershipRepository,
};
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, MembershipId, PrincipalId, ResourceName,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateServiceMembershipHandler {
    repository: Arc<dyn IMembershipRepository>,
}

impl CreateServiceMembershipHandler {
    pub fn new(repository: Arc<dyn IMembershipRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<CreateServiceMembership> for CreateServiceMembershipHandler {
    fn execute(
        &self,
        command: CreateServiceMembership,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<MembershipMutationResult>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let name = match ResourceName::parse(command.name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let role = match MembershipRole::parse(&command.role) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "name": name.as_str(),
                "role": role.as_str(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!("organizations/{}/memberships", command.organization_id),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let now = Utc::now();
            let principal = IdentityPrincipal::create_service(PrincipalId::new(), name, now);
            let membership = Membership::create(
                MembershipId::new(),
                command.organization_id,
                principal.id,
                role,
                now,
            );
            let principal_event =
                PrincipalCreated::envelope(command.organization_id, &principal, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let membership_event = MembershipChanged::created(&membership, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match repository
                .create_service_membership(CreateServiceMembershipWrite {
                    principal,
                    membership,
                    events: [principal_event, membership_event],
                    actor_principal_id: command.actor_principal_id,
                    actor_is_platform_admin: command.actor_is_platform_admin,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(MembershipMutationResult {
                membership: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
