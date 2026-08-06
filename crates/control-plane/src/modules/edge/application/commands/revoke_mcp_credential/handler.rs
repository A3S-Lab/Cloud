use super::RevokeMcpCredential;
use crate::modules::edge::application::McpCredentialMutationResult;
use crate::modules::edge::domain::events::McpCredentialChanged;
use crate::modules::edge::domain::repositories::{
    IMcpCredentialLifecycleRepository, RevokeMcpCredentialWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct RevokeMcpCredentialHandler {
    credentials: Arc<dyn IMcpCredentialLifecycleRepository>,
}

impl RevokeMcpCredentialHandler {
    pub fn new(credentials: Arc<dyn IMcpCredentialLifecycleRepository>) -> Self {
        Self { credentials }
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
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            if command.expected_aggregate_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected MCP credential aggregate version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&CanonicalRevokeMcpCredential {
                organization_id: command.organization_id,
                credential_id: command.credential_id,
                expected_aggregate_version: command.expected_aggregate_version,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/mcp-credentials/{}/revoke",
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
                    return Ok(Ok(McpCredentialMutationResult {
                        credential: write.credential,
                        replayed: true,
                    }))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let mut credential = match credentials
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
            if credential.aggregate_version() != command.expected_aggregate_version {
                return Ok(Err(ApplicationError::Conflict(
                    "MCP credential changed before revocation".into(),
                )));
            }
            let changed = match credential.revoke(command.requested_at) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = if changed {
                Some(
                    McpCredentialChanged::revoked(&credential, command.request_id)
                        .map_err(|error| BootError::Internal(error.to_string()))?,
                )
            } else {
                None
            };
            let write = match credentials
                .revoke_mcp_credential(RevokeMcpCredentialWrite {
                    credential,
                    expected_aggregate_version: command.expected_aggregate_version,
                    idempotency,
                    request_id: command.request_id,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(McpCredentialMutationResult {
                credential: write.credential,
                replayed: write.replayed,
            }))
        })
    }
}

#[derive(Serialize)]
struct CanonicalRevokeMcpCredential {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    credential_id: crate::modules::shared_kernel::domain::McpCredentialId,
    expected_aggregate_version: u64,
}
