use super::{ConnectorRevision, ConnectorRevisionRevocation, ConnectorRevisionRevoked};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokeConnectorRevisionWrite {
    pub revocation: ConnectorRevisionRevocation,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl RevokeConnectorRevisionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.revocation.validate()?;
        self.idempotency.validate()?;
        if self.actor_principal_id != self.revocation.revoked_by || self.request_id.is_nil() {
            return Err("Connector revision revocation write identity is invalid".into());
        }
        validate_event(&self.event, &self.revocation, self.request_id)
    }

    pub fn validate_against(&self, revision: &ConnectorRevision) -> Result<(), String> {
        self.validate()?;
        self.revocation.validate_against(revision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConnectorRevisionRevocationReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
}

impl From<&ConnectorRevisionRevocation> for ConnectorRevisionRevocationReference {
    fn from(revocation: &ConnectorRevisionRevocation) -> Self {
        Self {
            organization_id: revocation.organization_id,
            project_id: revocation.project_id,
            environment_id: revocation.environment_id,
            profile_id: revocation.profile_id,
            revision_id: revocation.revision_id,
        }
    }
}

#[async_trait]
pub trait IConnectorRevisionRevocationRepository: Send + Sync {
    async fn replay_revocation_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ConnectorRevisionRevocation>, RepositoryError>;

    async fn revoke_revision(
        &self,
        write: RevokeConnectorRevisionWrite,
    ) -> Result<IdempotentWrite<ConnectorRevisionRevocation>, RepositoryError>;

    #[allow(clippy::too_many_arguments)]
    async fn find_revision_revocation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
    ) -> Result<Option<ConnectorRevisionRevocation>, RepositoryError>;
}

fn validate_event(
    event: &DomainEventEnvelope,
    revocation: &ConnectorRevisionRevocation,
    request_id: Uuid,
) -> Result<(), String> {
    if event.event_key != "connector.revision.revoked"
        || event.schema_version != 1
        || event.organization_id != revocation.organization_id.as_uuid()
        || event.aggregate_id != revocation.revision_id.as_uuid()
        || event.aggregate_version != 1
        || event.occurred_at != revocation.revoked_at
        || event.correlation_id != request_id
        || event.causation_id.is_some()
    {
        return Err("Connector revision revocation event is inconsistent".into());
    }
    let payload: ConnectorRevisionRevoked = serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("Connector revision revocation event is invalid: {error}"))?;
    if !payload.matches(revocation) {
        return Err("Connector revision revocation event payload is inconsistent".into());
    }
    Ok(())
}
