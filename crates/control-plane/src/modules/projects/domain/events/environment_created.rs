use crate::modules::projects::domain::entities::Environment;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentCreated {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub name: String,
}

impl EnvironmentCreated {
    pub fn envelope(
        environment: &Environment,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "project.environment.created".into(),
            schema_version: 1,
            organization_id: environment.organization_id.as_uuid(),
            aggregate_id: environment.id.as_uuid(),
            aggregate_version: environment.aggregate_version,
            occurred_at: environment.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: environment.organization_id,
                project_id: environment.project_id,
                environment_id: environment.id,
                name: environment.name.as_str().to_owned(),
            })?,
        })
    }
}
