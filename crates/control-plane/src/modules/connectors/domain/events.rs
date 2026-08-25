use super::{
    ConnectorExecutionAttemptResolution, ConnectorProfile, ConnectorRevision,
    ConnectorRevisionRevocation,
};
use crate::modules::shared_kernel::domain::{
    ConnectorRevisionId, EnvironmentId, PrincipalId, ProjectId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorRevisionPublished {
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub profile_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub definition_kind: String,
    pub definition_schema: String,
    pub definition_digest: String,
    pub secret_binding_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorRevisionRevoked {
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: crate::modules::shared_kernel::domain::ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub revision_number: u64,
    pub definition_digest: String,
    pub reason: String,
    pub revoked_by: PrincipalId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorExecutionAttemptResolved {
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: crate::modules::shared_kernel::domain::ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub attempt_id: Uuid,
    pub request_digest: String,
    pub request_body_bytes: u64,
    pub resolution: String,
    pub reason: String,
    pub resolved_by: PrincipalId,
}

impl ConnectorExecutionAttemptResolved {
    pub fn envelope(
        resolution: &ConnectorExecutionAttemptResolution,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let binding = resolution.binding();
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "connector.execution-attempt.resolved".into(),
            schema_version: 1,
            organization_id: binding.organization_id().as_uuid(),
            aggregate_id: binding.attempt_id(),
            aggregate_version: 1,
            occurred_at: resolution.resolved_at(),
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self::from(resolution))?,
        })
    }

    pub fn matches(&self, resolution: &ConnectorExecutionAttemptResolution) -> bool {
        let binding = resolution.binding();
        self.project_id == binding.project_id()
            && self.environment_id == binding.environment_id()
            && self.profile_id == binding.profile_id()
            && self.revision_id == binding.revision_id()
            && self.attempt_id == binding.attempt_id()
            && self.request_digest == binding.request_digest().as_str()
            && self.request_body_bytes == binding.request_body_bytes()
            && self.resolution == "indeterminate"
            && self.reason == resolution.reason()
            && self.resolved_by == resolution.resolved_by()
    }
}

impl From<&ConnectorExecutionAttemptResolution> for ConnectorExecutionAttemptResolved {
    fn from(resolution: &ConnectorExecutionAttemptResolution) -> Self {
        let binding = resolution.binding();
        Self {
            project_id: binding.project_id(),
            environment_id: binding.environment_id(),
            profile_id: binding.profile_id(),
            revision_id: binding.revision_id(),
            attempt_id: binding.attempt_id(),
            request_digest: binding.request_digest().to_string(),
            request_body_bytes: binding.request_body_bytes(),
            resolution: "indeterminate".into(),
            reason: resolution.reason().to_owned(),
            resolved_by: resolution.resolved_by(),
        }
    }
}

impl ConnectorRevisionRevoked {
    pub fn envelope(
        revocation: &ConnectorRevisionRevocation,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self::from(revocation);
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "connector.revision.revoked".into(),
            schema_version: 1,
            organization_id: revocation.organization_id.as_uuid(),
            aggregate_id: revocation.revision_id.as_uuid(),
            aggregate_version: 1,
            occurred_at: revocation.revoked_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }

    pub fn matches(&self, revocation: &ConnectorRevisionRevocation) -> bool {
        self.project_id == revocation.project_id
            && self.environment_id == revocation.environment_id
            && self.profile_id == revocation.profile_id
            && self.revision_id == revocation.revision_id
            && self.revision_number == revocation.revision_number
            && self.definition_digest == revocation.definition_digest.as_str()
            && self.reason == revocation.reason
            && self.revoked_by == revocation.revoked_by
    }
}

impl From<&ConnectorRevisionRevocation> for ConnectorRevisionRevoked {
    fn from(revocation: &ConnectorRevisionRevocation) -> Self {
        Self {
            project_id: revocation.project_id,
            environment_id: revocation.environment_id,
            profile_id: revocation.profile_id,
            revision_id: revocation.revision_id,
            revision_number: revocation.revision_number,
            definition_digest: revocation.definition_digest.to_string(),
            reason: revocation.reason.clone(),
            revoked_by: revocation.revoked_by,
        }
    }
}

impl ConnectorRevisionPublished {
    pub fn created(
        profile: &ConnectorProfile,
        revision: &ConnectorRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "connector.profile.created",
            profile,
            revision,
            correlation_id,
        )
    }

    pub fn revised(
        profile: &ConnectorProfile,
        revision: &ConnectorRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "connector.profile.revised",
            profile,
            revision,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &'static str,
        profile: &ConnectorProfile,
        revision: &ConnectorRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: profile.project_id.as_uuid(),
            environment_id: profile.environment_id.as_uuid(),
            profile_id: profile.id.as_uuid(),
            revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            parent_revision_id: revision.parent_revision_id.map(|id| id.as_uuid()),
            definition_kind: revision.definition.kind().into(),
            definition_schema: revision.definition.schema().into(),
            definition_digest: revision.definition.digest().to_string(),
            secret_binding_count: revision.definition.secret_bindings().len(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: profile.organization_id.as_uuid(),
            aggregate_id: profile.id.as_uuid(),
            aggregate_version: profile.aggregate_version,
            occurred_at: revision.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
