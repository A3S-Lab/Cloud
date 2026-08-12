use super::RevokeResourceGrant;
use crate::modules::identity::application::ResourceGrantMutationResult;
use crate::modules::identity::domain::repositories::{
    IResourceGrantRepository, RevokeResourceGrantWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct RevokeResourceGrantHandler {
    repository: Arc<dyn IResourceGrantRepository>,
}

impl RevokeResourceGrantHandler {
    pub fn new(repository: Arc<dyn IResourceGrantRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<RevokeResourceGrant> for RevokeResourceGrantHandler {
    fn execute(
        &self,
        command: RevokeResourceGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ResourceGrantMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected Resource Grant version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "resourceGrantId": command.resource_grant_id,
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/resource-grants/{}/revocation",
                    command.organization_id, command.resource_grant_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .revoke_resource_grant(RevokeResourceGrantWrite {
                    organization_id: command.organization_id,
                    resource_grant_id: command.resource_grant_id,
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
            Ok(Ok(ResourceGrantMutationResult {
                resource_grant: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
