use crate::modules::executions::domain::ExecutionTemplateRevision;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTemplatePublished {
    pub project_id: Uuid,
    pub template_id: Uuid,
    pub revision_id: Uuid,
    pub definition_digest: String,
    pub capability: String,
}

impl ExecutionTemplatePublished {
    pub fn envelope(
        revision: &ExecutionTemplateRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: revision.project_id.as_uuid(),
            template_id: revision.template_id.as_uuid(),
            revision_id: revision.revision_id.as_uuid(),
            definition_digest: revision.definition.digest().to_string(),
            capability: crate::modules::executions::domain::EXECUTION_TEMPLATE_CAPABILITY.into(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "execution.template.published".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: revision.organization_id.as_uuid(),
            },
            aggregate_id: revision.template_id.as_uuid(),
            aggregate_version: 1,
            occurred_at: revision.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
