use super::IMcpCredentialRepository;
use crate::modules::edge::domain::events::McpCredentialChanged;
use crate::modules::edge::domain::{McpCredential, McpCredentialDelivery};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, McpCredentialId, OrganizationId,
    ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH: usize = 10_000;

#[derive(Debug, Clone)]
pub struct StoreMcpCredentialLifecycle {
    pub credential: McpCredential,
    pub expected_aggregate_version: Option<u64>,
    pub delivery: Option<McpCredentialDelivery>,
    pub observed_at: DateTime<Utc>,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

impl StoreMcpCredentialLifecycle {
    pub(crate) fn validate(&self) -> Result<(), RepositoryError> {
        let expected_event_key = if self.credential.revoked_at().is_some() {
            "edge.mcp-credential.revoked"
        } else if self.expected_aggregate_version.is_none() {
            "edge.mcp-credential.issued"
        } else {
            "edge.mcp-credential.rotated"
        };
        let initial = self.expected_aggregate_version.is_none()
            && self.credential.generation() == 1
            && self.credential.aggregate_version() == 1
            && self.credential.created_at() == self.credential.updated_at()
            && self.credential.revoked_at().is_none();
        let transition = self.expected_aggregate_version.is_some_and(|expected| {
            expected > 0
                && expected.checked_add(1) == Some(self.credential.aggregate_version())
                && self.credential.generation() > 0
        });
        let delivery_matches = match &self.delivery {
            Some(delivery) => {
                delivery.validate().map_err(RepositoryError::Conflict)?;
                delivery.matches_credential(&self.credential)
                    && delivery.is_available_at(self.observed_at)
                    && self.credential.revoked_at().is_none()
            }
            None => self.credential.revoked_at().is_some(),
        };
        let expected_payload = McpCredentialChanged {
            organization_id: self.credential.organization_id,
            project_id: self.credential.project_id,
            environment_id: self.credential.environment_id,
            credential_id: self.credential.id,
            generation: self.credential.generation(),
            expires_at: self.credential.expires_at(),
            revoked: self.credential.revoked_at().is_some(),
        };
        let payload_matches =
            serde_json::from_value::<McpCredentialChanged>(self.event.payload.clone())
                .is_ok_and(|payload| payload == expected_payload);
        if (!initial && !transition)
            || !delivery_matches
            || self.observed_at != canonical_timestamp(self.observed_at)
            || self.observed_at < self.credential.updated_at()
            || self.event.organization_id != self.credential.organization_id.as_uuid()
            || self.event.aggregate_id != self.credential.id.as_uuid()
            || self.event.aggregate_version != self.credential.aggregate_version()
            || self.event.occurred_at != self.credential.updated_at()
            || self.event.correlation_id.is_nil()
            || self.event.event_id.is_nil()
            || self.event.schema_version != 1
            || self.event.event_key != expected_event_key
            || !payload_matches
        {
            return Err(RepositoryError::Conflict(
                "MCP credential lifecycle bundle is inconsistent".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCredentialLifecycleResult {
    pub credential: McpCredential,
    pub delivery: Option<McpCredentialDelivery>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct McpCredentialLifecycleReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub credential_id: McpCredentialId,
    pub generation: u64,
    pub aggregate_version: u64,
    pub has_delivery: bool,
}

impl McpCredentialLifecycleReference {
    pub(crate) fn from_bundle(bundle: &StoreMcpCredentialLifecycle) -> Self {
        Self {
            organization_id: bundle.credential.organization_id,
            project_id: bundle.credential.project_id,
            environment_id: bundle.credential.environment_id,
            credential_id: bundle.credential.id,
            generation: bundle.credential.generation(),
            aggregate_version: bundle.credential.aggregate_version(),
            has_delivery: bundle.delivery.is_some(),
        }
    }

    pub(crate) fn matches_credential(&self, credential: &McpCredential) -> bool {
        self.organization_id == credential.organization_id
            && self.project_id == credential.project_id
            && self.environment_id == credential.environment_id
            && self.credential_id == credential.id
            && self.generation == credential.generation()
            && self.aggregate_version == credential.aggregate_version()
            && self.has_delivery == credential.revoked_at().is_none()
    }
}

#[async_trait]
pub trait IMcpCredentialLifecycleRepository: Send + Sync {
    /// Replays only the exact current credential generation. If its encrypted
    /// recovery window has expired or the lifecycle advanced, the repository
    /// must fail closed and must never generate or return replacement material.
    async fn replay_mcp_credential_lifecycle(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        idempotency: &IdempotencyRequest,
        observed_at: DateTime<Utc>,
    ) -> Result<Option<McpCredentialLifecycleResult>, RepositoryError>;

    /// Atomically stores one issuance, rotation, or revocation together with
    /// its outbox event, idempotency reference, and current recovery material.
    async fn store_mcp_credential_lifecycle(
        &self,
        bundle: StoreMcpCredentialLifecycle,
    ) -> Result<McpCredentialLifecycleResult, RepositoryError>;

    /// Removes expired encrypted recovery material without removing the
    /// idempotency reference that prevents a second secret from being minted.
    async fn purge_expired_mcp_credential_deliveries(
        &self,
        observed_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError>;
}

pub trait IMcpCredentialAuthorityRepository:
    IMcpCredentialRepository + IMcpCredentialLifecycleRepository
{
}

impl<T> IMcpCredentialAuthorityRepository for T where
    T: IMcpCredentialRepository + IMcpCredentialLifecycleRepository
{
}
