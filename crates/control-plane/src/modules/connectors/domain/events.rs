use super::{ConnectorProfile, ConnectorRevision};
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
