use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::entities::ExternalSourceRevision;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRevisionAccepted {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
    pub repository_identity: String,
    pub commit_sha: String,
    pub recipe_digest: String,
}

impl SourceRevisionAccepted {
    pub fn envelope(
        revision: &ExternalSourceRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "source.revision.accepted".into(),
            schema_version: 1,
            organization_id: revision.organization_id.as_uuid(),
            aggregate_id: revision.id.as_uuid(),
            aggregate_version: revision.aggregate_version,
            occurred_at: revision.accepted_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: revision.organization_id,
                project_id: revision.project_id,
                environment_id: revision.environment_id,
                source_revision_id: revision.id,
                repository_identity: revision.repository.identity().to_owned(),
                commit_sha: revision.commit_sha.as_str().to_owned(),
                recipe_digest: revision.recipe_digest.clone(),
            })?,
        })
    }
}
