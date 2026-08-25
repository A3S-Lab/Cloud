use crate::modules::sources::domain::entities::ExternalSourceRevision;
use crate::modules::sources::published::{
    SourceRevisionAcceptedFact, SOURCE_REVISION_ACCEPTED_EVENT_KEY,
    SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use uuid::Uuid;

pub struct SourceRevisionAccepted;

impl SourceRevisionAccepted {
    pub fn envelope(
        revision: &ExternalSourceRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let fact = SourceRevisionAcceptedFact::new(
            revision.organization_id,
            revision.project_id,
            revision.environment_id,
            revision.id,
            revision.repository.identity().to_owned(),
            revision.commit_sha.as_str().to_owned(),
            revision.recipe_digest.clone(),
        );
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: SOURCE_REVISION_ACCEPTED_EVENT_KEY.into(),
            schema_version: SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION,
            organization_id: revision.organization_id.as_uuid(),
            aggregate_id: revision.id.as_uuid(),
            aggregate_version: revision.aggregate_version,
            occurred_at: revision.accepted_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(fact)?,
        })
    }
}
