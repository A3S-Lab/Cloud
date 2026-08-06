use super::RotateMcpCredential;
use crate::modules::edge::application::commands::create_mcp_credential::{
    identity_collision, issuance_error,
};
use crate::modules::edge::application::{encrypt_delivery_receipt, recover_delivery};
use crate::modules::edge::domain::events::McpCredentialChanged;
use crate::modules::edge::domain::repositories::{
    IMcpCredentialLifecycleRepository, RotateMcpCredentialWrite,
};
use crate::modules::edge::domain::services::IMcpCredentialIssuer;
use crate::modules::secrets::domain::ISecretEncryptionService;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

const MAX_IDENTITY_ATTEMPTS: usize = 4;

pub struct RotateMcpCredentialHandler {
    credentials: Arc<dyn IMcpCredentialLifecycleRepository>,
    issuer: Arc<dyn IMcpCredentialIssuer>,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl RotateMcpCredentialHandler {
    pub fn new(
        credentials: Arc<dyn IMcpCredentialLifecycleRepository>,
        issuer: Arc<dyn IMcpCredentialIssuer>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            credentials,
            issuer,
            encryption,
        }
    }
}

impl CommandHandler<RotateMcpCredential> for RotateMcpCredentialHandler {
    fn execute(
        &self,
        command: RotateMcpCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<
            ApplicationResult<crate::modules::edge::application::McpCredentialDeliveryResult>,
        >,
    > {
        let credentials = Arc::clone(&self.credentials);
        let issuer = Arc::clone(&self.issuer);
        let encryption = Arc::clone(&self.encryption);
        Box::pin(async move {
            if command.expected_aggregate_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected MCP credential aggregate version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&CanonicalRotateMcpCredential {
                organization_id: command.organization_id,
                credential_id: command.credential_id,
                expires_at: command.expires_at,
                expected_aggregate_version: command.expected_aggregate_version,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/mcp-credentials/{}/rotate",
                    command.organization_id, command.credential_id
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
            let existing = match credentials
                .find_mcp_credential(command.organization_id, command.credential_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "MCP credential not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if existing.aggregate_version() != command.expected_aggregate_version {
                return Ok(Err(ApplicationError::Conflict(
                    "MCP credential changed before rotation".into(),
                )));
            }

            for attempt in 0..MAX_IDENTITY_ATTEMPTS {
                let issued = match issuer
                    .rotate(existing.clone(), command.expires_at, command.requested_at)
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
                let event = McpCredentialChanged::rotated(&credential, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
                match credentials
                    .rotate_mcp_credential_delivery(RotateMcpCredentialWrite {
                        credential,
                        receipt,
                        expected_aggregate_version: command.expected_aggregate_version,
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
                            "MCP credential rotation exhausted its bounded identity retries".into(),
                        )))
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            Ok(Err(ApplicationError::Unavailable(
                "MCP credential rotation is unavailable".into(),
            )))
        })
    }
}

#[derive(Serialize)]
struct CanonicalRotateMcpCredential {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    credential_id: crate::modules::shared_kernel::domain::McpCredentialId,
    expires_at: chrono::DateTime<chrono::Utc>,
    expected_aggregate_version: u64,
}
