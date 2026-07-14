use crate::modules::projects::domain::entities::Project;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCreated {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub name: String,
}

impl ProjectCreated {
    pub fn envelope(
        project: &Project,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "project.project.created".into(),
            schema_version: 1,
            organization_id: project.organization_id.as_uuid(),
            aggregate_id: project.id.as_uuid(),
            aggregate_version: project.aggregate_version,
            occurred_at: project.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: project.organization_id,
                project_id: project.id,
                name: project.name.as_str().to_owned(),
            })?,
        })
    }
}
