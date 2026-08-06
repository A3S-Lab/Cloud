use super::CreateMcpCredential;
use crate::modules::edge::application::{encrypt_delivery_receipt, recover_delivery};
use crate::modules::edge::domain::events::McpCredentialChanged;
use crate::modules::edge::domain::repositories::{
    CreateMcpCredentialWrite, IMcpCredentialLifecycleRepository,
};
use crate::modules::edge::domain::services::{
    IMcpCredentialIssuer, McpCredentialIssuanceError, McpCredentialIssueRequest,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::secrets::domain::ISecretEncryptionService;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

const MAX_IDENTITY_ATTEMPTS: usize = 4;

pub struct CreateMcpCredentialHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    credentials: Arc<dyn IMcpCredentialLifecycleRepository>,
    issuer: Arc<dyn IMcpCredentialIssuer>,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl CreateMcpCredentialHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        credentials: Arc<dyn IMcpCredentialLifecycleRepository>,
        issuer: Arc<dyn IMcpCredentialIssuer>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            environments,
            credentials,
            issuer,
            encryption,
        }
    }
}

impl CommandHandler<CreateMcpCredential> for CreateMcpCredentialHandler {
    fn execute(
        &self,
        command: CreateMcpCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<
            ApplicationResult<crate::modules::edge::application::McpCredentialDeliveryResult>,
        >,
    > {
        let environments = Arc::clone(&self.environments);
        let credentials = Arc::clone(&self.credentials);
        let issuer = Arc::clone(&self.issuer);
        let encryption = Arc::clone(&self.encryption);
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
            let canonical = serde_json::to_vec(&CanonicalCreateMcpCredential {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                expires_at: command.expires_at,
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
            match credentials
                .replay_mcp_credential_write(command.organization_id, &idempotency)
                .await
            {
                Ok(Some(write)) => {
                    return Ok(
                        recover_delivery(encryption.as_ref(), write, command.requested_at).await,
                    )
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            for attempt in 0..MAX_IDENTITY_ATTEMPTS {
                let issued = match issuer
                    .issue(McpCredentialIssueRequest {
                        organization_id: command.organization_id,
                        project_id: command.project_id,
                        environment_id: command.environment_id,
                        expires_at: command.expires_at,
                        issued_at: command.requested_at,
                    })
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(issuance_error(error))),
                };
                let (credential, bearer) = issued.into_parts();
                let receipt = match encrypt_delivery_receipt(
                    encryption.as_ref(),
                    &credential,
                    bearer.as_str(),
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
                let event = McpCredentialChanged::created(&credential, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
                match credentials
                    .create_mcp_credential_delivery(CreateMcpCredentialWrite {
                        credential,
                        receipt,
                        idempotency: idempotency.clone(),
                        event,
                    })
                    .await
                {
                    Ok(write) => {
                        return Ok(recover_delivery(
                            encryption.as_ref(),
                            write,
                            command.requested_at,
                        )
                        .await)
                    }
                    Err(error)
                        if identity_collision(&error) && attempt + 1 < MAX_IDENTITY_ATTEMPTS => {}
                    Err(error) if identity_collision(&error) => {
                        return Ok(Err(ApplicationError::Unavailable(
                            "MCP credential issuance exhausted its bounded identity retries".into(),
                        )))
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            Ok(Err(ApplicationError::Unavailable(
                "MCP credential issuance is unavailable".into(),
            )))
        })
    }
}

#[derive(Serialize)]
struct CanonicalCreateMcpCredential {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    expires_at: chrono::DateTime<chrono::Utc>,
}

pub(super) fn issuance_error(error: McpCredentialIssuanceError) -> ApplicationError {
    match error {
        McpCredentialIssuanceError::InvalidRequest(message) => ApplicationError::Invalid(message),
        McpCredentialIssuanceError::Unavailable => {
            ApplicationError::Unavailable("MCP credential issuance is unavailable".into())
        }
    }
}

pub(super) fn identity_collision(error: &RepositoryError) -> bool {
    matches!(
        error,
        RepositoryError::Conflict(message)
            if message.contains("MCP credential")
                && (message.contains("lookup prefix") || message.contains("identity"))
    )
}
