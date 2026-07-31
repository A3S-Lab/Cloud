use crate::modules::edge::application::{
    McpCredentialLifecycleService, McpCredentialMutationResult,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, McpCredentialId, OrganizationId,
    ProjectId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RotateMcpCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub credential_id: McpCredentialId,
    pub expires_at: DateTime<Utc>,
    pub actor_id: Uuid,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for RotateMcpCredential {
    type Output = ApplicationResult<McpCredentialMutationResult>;
}

pub struct RotateMcpCredentialHandler {
    lifecycle: McpCredentialLifecycleService,
}

impl RotateMcpCredentialHandler {
    pub const fn new(lifecycle: McpCredentialLifecycleService) -> Self {
        Self { lifecycle }
    }
}

impl CommandHandler<RotateMcpCredential> for RotateMcpCredentialHandler {
    fn execute(
        &self,
        command: RotateMcpCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<McpCredentialMutationResult>>,
    > {
        let lifecycle = self.lifecycle.clone();
        Box::pin(async move {
            let expires_at = canonical_timestamp(command.expires_at);
            let canonical = serde_json::to_vec(&CanonicalRotateMcpCredential {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                credential_id: command.credential_id,
                expires_at,
                actor_id: command.actor_id,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/mcp-credentials/{}/rotate",
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
            let credential = match lifecycle
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
            Ok(lifecycle
                .rotate(
                    &credential,
                    expires_at,
                    command.requested_at,
                    idempotency,
                    command.actor_id,
                    command.request_id,
                )
                .await)
        })
    }
}

#[derive(Serialize)]
struct CanonicalRotateMcpCredential {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    credential_id: McpCredentialId,
    expires_at: DateTime<Utc>,
    actor_id: Uuid,
}
