use crate::modules::projects::domain::value_objects::ProjectName;
use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId, ProjectId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub organization_id: OrganizationId,
    pub id: ProjectId,
    pub name: ProjectName,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
}

impl Project {
    pub fn create(
        organization_id: OrganizationId,
        id: ProjectId,
        name: ProjectName,
        created_at: DateTime<Utc>,
    ) -> Self {
        let created_at = canonical_timestamp(created_at);
        Self {
            organization_id,
            id,
            name,
            aggregate_version: 1,
            created_at,
        }
    }
}
