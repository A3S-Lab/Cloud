use crate::modules::workflow::domain::{Ontology, OntologyRevision};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyRevisionPublished {
    pub project_id: Uuid,
    pub ontology_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub content_digest: String,
    pub compiler_schema_version: u32,
    pub migration_policy: String,
    pub migration_rule_id: Option<String>,
}

impl OntologyRevisionPublished {
    pub fn created(
        ontology: &Ontology,
        revision: &OntologyRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "workflow.ontology.created",
            ontology,
            revision,
            correlation_id,
        )
    }

    pub fn revised(
        ontology: &Ontology,
        revision: &OntologyRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "workflow.ontology.revised",
            ontology,
            revision,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &'static str,
        ontology: &Ontology,
        revision: &OntologyRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: ontology.project_id.as_uuid(),
            ontology_id: ontology.id.as_uuid(),
            revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            parent_revision_id: revision.parent_revision_id.map(|id| id.as_uuid()),
            content_digest: revision.contract.digest().as_str().to_owned(),
            compiler_schema_version: revision.compiler_schema_version,
            migration_policy: revision.migration_policy.kind().to_owned(),
            migration_rule_id: revision.migration_policy.rule_id().map(str::to_owned),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: ontology.organization_id.as_uuid(),
            },
            aggregate_id: ontology.id.as_uuid(),
            aggregate_version: ontology.aggregate_version,
            occurred_at: revision.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
