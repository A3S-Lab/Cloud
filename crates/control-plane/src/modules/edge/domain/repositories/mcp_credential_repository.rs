use crate::modules::edge::domain::events::McpCredentialChanged;
use crate::modules::edge::domain::{McpCredential, McpCredentialDeliveryReceipt};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, McpCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(crate) const MAX_MCP_CREDENTIAL_RESOLUTION_BATCH: usize = 10_000;

pub(crate) fn validate_mcp_credential_resolution(
    credential_ids: &[McpCredentialId],
) -> Result<(), RepositoryError> {
    if credential_ids.len() > MAX_MCP_CREDENTIAL_RESOLUTION_BATCH
        || credential_ids
            .iter()
            .any(|credential_id| credential_id.as_uuid().is_nil())
        || credential_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != credential_ids.len()
    {
        return Err(RepositoryError::Conflict(
            "MCP credential resolution requires at most 10000 unique non-nil identities".into(),
        ));
    }
    Ok(())
}

#[async_trait]
pub trait IMcpCredentialRepository: Send + Sync {
    async fn create_mcp_credential(
        &self,
        credential: McpCredential,
    ) -> Result<McpCredential, RepositoryError>;

    async fn update_mcp_credential(
        &self,
        credential: McpCredential,
        expected_aggregate_version: u64,
    ) -> Result<McpCredential, RepositoryError>;

    async fn find_mcp_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: McpCredentialId,
    ) -> Result<Option<McpCredential>, RepositoryError>;

    async fn list_mcp_credentials(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<McpCredential>, RepositoryError>;

    /// Resolves only the requested credential identities within one exact
    /// tenant scope. Missing or cross-scope identities are intentionally
    /// omitted so callers can apply tenant non-disclosure.
    async fn resolve_mcp_credentials(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        credential_ids: &[McpCredentialId],
    ) -> Result<Vec<McpCredential>, RepositoryError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCredentialWriteReference {
    pub credential_id: McpCredentialId,
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct McpCredentialWrite {
    pub credential: McpCredential,
    pub receipt: Option<McpCredentialDeliveryReceipt>,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct CreateMcpCredentialWrite {
    pub credential: McpCredential,
    pub receipt: McpCredentialDeliveryReceipt,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct RotateMcpCredentialWrite {
    pub credential: McpCredential,
    pub receipt: McpCredentialDeliveryReceipt,
    pub expected_aggregate_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct RevokeMcpCredentialWrite {
    pub credential: McpCredential,
    pub expected_aggregate_version: u64,
    pub idempotency: IdempotencyRequest,
    pub request_id: uuid::Uuid,
    pub event: Option<DomainEventEnvelope>,
}

impl CreateMcpCredentialWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.credential.generation() != 1
            || self.credential.aggregate_version() != 1
            || self.credential.created_at() != self.credential.updated_at()
            || self.credential.revoked_at().is_some()
        {
            return Err("new MCP credential is not at its initial generation".into());
        }
        self.receipt.validate_against(&self.credential)?;
        validate_event(&self.credential, &self.event, "edge.mcp-credential.created")
    }
}

impl RotateMcpCredentialWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.expected_aggregate_version == 0
            || self.expected_aggregate_version.checked_add(1)
                != Some(self.credential.aggregate_version())
            || self.credential.generation() < 2
            || self.credential.revoked_at().is_some()
        {
            return Err("MCP credential rotation version is invalid".into());
        }
        self.receipt.validate_against(&self.credential)?;
        validate_event(&self.credential, &self.event, "edge.mcp-credential.rotated")
    }
}

impl RevokeMcpCredentialWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.expected_aggregate_version == 0
            || self.request_id.is_nil()
            || self.credential.revoked_at().is_none()
        {
            return Err("MCP credential revocation is invalid".into());
        }
        match &self.event {
            Some(event)
                if self.expected_aggregate_version.checked_add(1)
                    == Some(self.credential.aggregate_version()) =>
            {
                validate_event(&self.credential, event, "edge.mcp-credential.revoked")
            }
            None if self.expected_aggregate_version == self.credential.aggregate_version() => {
                Ok(())
            }
            _ => Err("MCP credential revocation event version is invalid".into()),
        }
    }
}

#[async_trait]
pub trait IMcpCredentialLifecycleRepository: IMcpCredentialRepository {
    async fn replay_mcp_credential_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<McpCredentialWrite>, RepositoryError>;

    async fn create_mcp_credential_delivery(
        &self,
        bundle: CreateMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError>;

    async fn rotate_mcp_credential_delivery(
        &self,
        bundle: RotateMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError>;

    async fn revoke_mcp_credential(
        &self,
        bundle: RevokeMcpCredentialWrite,
    ) -> Result<McpCredentialWrite, RepositoryError>;

    /// Permanently removes expired one-time delivery material while leaving
    /// the credential aggregate and its idempotency record intact.
    async fn sweep_expired_mcp_credential_delivery_receipts(
        &self,
        expired_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError>;
}

fn validate_event(
    credential: &McpCredential,
    event: &DomainEventEnvelope,
    event_key: &str,
) -> Result<(), String> {
    if event.event_key != event_key
        || event.organization_id() != Some(credential.organization_id.as_uuid())
        || event.aggregate_id != credential.id.as_uuid()
        || event.aggregate_version != credential.aggregate_version()
        || event.occurred_at != credential.updated_at()
    {
        return Err("MCP credential write and domain event are inconsistent".into());
    }
    let payload: McpCredentialChanged = serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("MCP credential domain event is invalid: {error}"))?;
    if payload.organization_id != credential.organization_id
        || payload.project_id != credential.project_id
        || payload.environment_id != credential.environment_id
        || payload.credential_id != credential.id
        || payload.generation != credential.generation()
        || payload.expires_at != credential.expires_at()
        || payload.state
            != if credential.revoked_at().is_some() {
                "revoked"
            } else {
                "active"
            }
    {
        return Err("MCP credential domain event payload is inconsistent".into());
    }
    Ok(())
}
