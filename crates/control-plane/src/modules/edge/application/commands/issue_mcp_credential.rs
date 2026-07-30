use crate::modules::edge::application::{
    McpCredentialLifecycleService, McpCredentialMutationResult,
};
use crate::modules::edge::infrastructure::McpCredentialIssueRequest;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct IssueMcpCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub expires_at: DateTime<Utc>,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for IssueMcpCredential {
    type Output = ApplicationResult<McpCredentialMutationResult>;
}

pub struct IssueMcpCredentialHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    lifecycle: McpCredentialLifecycleService,
}

impl IssueMcpCredentialHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        lifecycle: McpCredentialLifecycleService,
    ) -> Self {
        Self {
            environments,
            lifecycle,
        }
    }
}

impl CommandHandler<IssueMcpCredential> for IssueMcpCredentialHandler {
    fn execute(
        &self,
        command: IssueMcpCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<McpCredentialMutationResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let lifecycle = self.lifecycle.clone();
        Box::pin(async move {
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let expires_at = canonical_timestamp(command.expires_at);
            let canonical = serde_json::to_vec(&CanonicalIssueMcpCredential {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                expires_at,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/mcp-credentials",
                    command.organization_id, command.project_id, command.environment_id
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
            Ok(lifecycle
                .issue(
                    McpCredentialIssueRequest {
                        organization_id: command.organization_id,
                        project_id: command.project_id,
                        environment_id: command.environment_id,
                        expires_at,
                        issued_at: command.requested_at,
                    },
                    idempotency,
                    command.request_id,
                )
                .await)
        })
    }
}

#[derive(Serialize)]
struct CanonicalIssueMcpCredential {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    expires_at: DateTime<Utc>,
}
