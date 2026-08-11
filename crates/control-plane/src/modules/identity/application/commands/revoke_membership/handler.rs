use super::RevokeMembership;
use crate::modules::identity::application::MembershipMutationResult;
use crate::modules::identity::domain::repositories::{
    IMembershipRepository, RevokeMembershipWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct RevokeMembershipHandler {
    repository: Arc<dyn IMembershipRepository>,
}

impl RevokeMembershipHandler {
    pub fn new(repository: Arc<dyn IMembershipRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<RevokeMembership> for RevokeMembershipHandler {
    fn execute(
        &self,
        command: RevokeMembership,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<MembershipMutationResult>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected membership version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "membershipId": command.membership_id,
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/memberships/{}/revocation",
                    command.organization_id, command.membership_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .revoke_membership(RevokeMembershipWrite {
                    organization_id: command.organization_id,
                    membership_id: command.membership_id,
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
            Ok(Ok(MembershipMutationResult {
                membership: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
