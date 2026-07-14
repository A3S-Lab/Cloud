use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: EnvironmentId,
    pub name: EnvironmentName,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
}

impl Environment {
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: EnvironmentId,
        name: EnvironmentName,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            id,
            name,
            aggregate_version: 1,
            created_at,
        }
    }
}
