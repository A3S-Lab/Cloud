use super::ChangeMembershipRole;
use crate::modules::identity::application::MembershipMutationResult;
use crate::modules::identity::domain::repositories::{
    ChangeMembershipRoleWrite, IMembershipRepository,
};
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct ChangeMembershipRoleHandler {
    repository: Arc<dyn IMembershipRepository>,
}

impl ChangeMembershipRoleHandler {
    pub fn new(repository: Arc<dyn IMembershipRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<ChangeMembershipRole> for ChangeMembershipRoleHandler {
    fn execute(
        &self,
        command: ChangeMembershipRole,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<MembershipMutationResult>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let role = match MembershipRole::parse(&command.role) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected membership version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "membershipId": command.membership_id,
                "role": role.as_str(),
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/memberships/{}/role",
                    command.organization_id, command.membership_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .change_membership_role(ChangeMembershipRoleWrite {
                    organization_id: command.organization_id,
                    membership_id: command.membership_id,
                    role,
                    expected_version: command.expected_version,
                    actor_principal_id: command.actor_principal_id,
                    changed_at: Utc::now(),
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
