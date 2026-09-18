use super::RevokeDirectoryResourceGrant;
use crate::modules::identity::application::DirectoryResourceGrantMutationResult;
use crate::modules::identity::domain::repositories::{
    IDirectoryResourceGrantRepository, RevokeDirectoryResourceGrantWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct RevokeDirectoryResourceGrantHandler {
    repository: Arc<dyn IDirectoryResourceGrantRepository>,
}

impl RevokeDirectoryResourceGrantHandler {
    pub fn new(repository: Arc<dyn IDirectoryResourceGrantRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<RevokeDirectoryResourceGrant> for RevokeDirectoryResourceGrantHandler {
    fn execute(
        &self,
        command: RevokeDirectoryResourceGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DirectoryResourceGrantMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected Directory Resource Grant version must be positive".into(),
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
                    "organizations/{}/directory-resource-grants/{}/revocation",
                    command.organization_id, command.resource_grant_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .revoke_directory_resource_grant(RevokeDirectoryResourceGrantWrite {
                    organization_id: command.organization_id,
                    resource_grant_id: command.resource_grant_id,
                    expected_version: command.expected_version,
                    actor_principal_id: command.actor_principal_id,
                    revoked_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(DirectoryResourceGrantMutationResult {
                directory_resource_grant: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
