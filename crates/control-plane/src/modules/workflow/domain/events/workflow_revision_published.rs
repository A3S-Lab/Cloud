use crate::modules::workflow::domain::{WorkflowDefinition, WorkflowRevision};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRevisionPublished {
    pub project_id: Uuid,
    pub workflow_definition_id: Uuid,
    pub workflow_revision_id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub content_digest: String,
    pub payload_set_digest: String,
    pub compiler_schema_version: u32,
}

impl WorkflowRevisionPublished {
    pub fn created(
        definition: &WorkflowDefinition,
        revision: &WorkflowRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "workflow.definition.created",
            definition,
            revision,
            correlation_id,
        )
    }

    pub fn revised(
        definition: &WorkflowDefinition,
        revision: &WorkflowRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "workflow.definition.revised",
            definition,
            revision,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &'static str,
        definition: &WorkflowDefinition,
        revision: &WorkflowRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: definition.project_id.as_uuid(),
            workflow_definition_id: definition.id.as_uuid(),
            workflow_revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            parent_revision_id: revision.parent_revision_id.map(|id| id.as_uuid()),
            content_digest: revision.contract.digest().as_str().to_owned(),
            payload_set_digest: revision.payload_set_digest.as_str().to_owned(),
            compiler_schema_version: revision.compiler_schema_version,
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: definition.organization_id.as_uuid(),
            aggregate_id: definition.id.as_uuid(),
            aggregate_version: definition.aggregate_version,
            occurred_at: revision.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
