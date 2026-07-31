use crate::modules::edge::application::{
    McpCredentialLifecycleService, McpCredentialMutationResult,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, McpCredentialId, OrganizationId, ProjectId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokeMcpCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub credential_id: McpCredentialId,
    pub actor_id: Uuid,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for RevokeMcpCredential {
    type Output = ApplicationResult<McpCredentialMutationResult>;
}

pub struct RevokeMcpCredentialHandler {
    lifecycle: McpCredentialLifecycleService,
}

impl RevokeMcpCredentialHandler {
    pub const fn new(lifecycle: McpCredentialLifecycleService) -> Self {
        Self { lifecycle }
    }
}

impl CommandHandler<RevokeMcpCredential> for RevokeMcpCredentialHandler {
    fn execute(
        &self,
        command: RevokeMcpCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<McpCredentialMutationResult>>,
    > {
        let lifecycle = self.lifecycle.clone();
        Box::pin(async move {
            let canonical = serde_json::to_vec(&CanonicalRevokeMcpCredential {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                credential_id: command.credential_id,
                actor_id: command.actor_id,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/mcp-credentials/{}/revoke",
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.credential_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match lifecycle
                .replay(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    &idempotency,
                    command.requested_at,
                )
                .await
            {
                Ok(Some(replay)) => return Ok(Ok(replay)),
                Ok(None) => {}
                Err(error) => return Ok(Err(error)),
            }
            let mut credential = match lifecycle
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.credential_id,
                )
                .await
            {
                Ok(credential) => credential,
                Err(error) => return Ok(Err(error)),
            };
            if credential.revoked_at().is_some() {
                return Ok(Ok(McpCredentialMutationResult::without_secret(
                    credential, true,
                )));
            }
            let expected_version = credential.aggregate_version();
            match credential.revoke(command.requested_at) {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(Ok(McpCredentialMutationResult::without_secret(
                        credential, true,
                    )))
                }
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            }
            Ok(lifecycle
                .revoke(
                    credential,
                    expected_version,
                    idempotency,
                    command.actor_id,
                    command.request_id,
                    command.requested_at,
                )
                .await)
        })
    }
}

#[derive(Serialize)]
struct CanonicalRevokeMcpCredential {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    credential_id: McpCredentialId,
    actor_id: Uuid,
}
