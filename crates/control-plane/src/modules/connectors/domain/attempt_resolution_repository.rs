use super::{
    ConnectorExecutionAttempt, ConnectorExecutionAttemptResolution,
    ConnectorExecutionAttemptResolved, ConnectorExecutionEvidence,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ResolveConnectorExecutionAttemptWrite {
    pub resolution: ConnectorExecutionAttemptResolution,
    pub evidence: ConnectorExecutionEvidence,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl ResolveConnectorExecutionAttemptWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.resolution.validate()?;
        self.resolution.validate_evidence(&self.evidence)?;
        self.idempotency.validate()?;
        if self.actor_principal_id != self.resolution.resolved_by() || self.request_id.is_nil() {
            return Err("Connector execution attempt resolution write identity is invalid".into());
        }
        validate_event(&self.event, &self.resolution, self.request_id)
    }

    pub fn validate_against(&self, attempt: &ConnectorExecutionAttempt) -> Result<(), String> {
        self.validate()?;
        self.resolution.validate_against(attempt)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConnectorExecutionAttemptResolutionReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub attempt_id: Uuid,
}

impl From<&ConnectorExecutionAttemptResolution> for ConnectorExecutionAttemptResolutionReference {
    fn from(resolution: &ConnectorExecutionAttemptResolution) -> Self {
        let binding = resolution.binding();
        Self {
            organization_id: binding.organization_id(),
            project_id: binding.project_id(),
            environment_id: binding.environment_id(),
            profile_id: binding.profile_id(),
            revision_id: binding.revision_id(),
            attempt_id: binding.attempt_id(),
        }
    }
}

#[async_trait]
pub trait IConnectorExecutionAttemptResolutionRepository: Send + Sync {
    async fn replay_resolution_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ConnectorExecutionAttemptResolution>, RepositoryError>;

    async fn resolve_indeterminate(
        &self,
        write: ResolveConnectorExecutionAttemptWrite,
    ) -> Result<IdempotentWrite<ConnectorExecutionAttemptResolution>, RepositoryError>;

    #[allow(clippy::too_many_arguments)]
    async fn find_resolution(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionAttemptResolution>, RepositoryError>;
}

fn validate_event(
    event: &DomainEventEnvelope,
    resolution: &ConnectorExecutionAttemptResolution,
    request_id: Uuid,
) -> Result<(), String> {
    let binding = resolution.binding();
    if event.event_key != "connector.execution-attempt.resolved"
        || event.schema_version != 1
        || event.organization_id() != Some(binding.organization_id().as_uuid())
        || event.aggregate_id != binding.attempt_id()
        || event.aggregate_version != 1
        || event.occurred_at != resolution.resolved_at()
        || event.correlation_id != request_id
        || event.causation_id.is_some()
    {
        return Err("Connector execution attempt resolution event is inconsistent".into());
    }
    let payload: ConnectorExecutionAttemptResolved = serde_json::from_value(event.payload.clone())
        .map_err(|error| {
            format!("Connector execution attempt resolution event is invalid: {error}")
        })?;
    if !payload.matches(resolution) {
        return Err("Connector execution attempt resolution event payload is inconsistent".into());
    }
    Ok(())
}
