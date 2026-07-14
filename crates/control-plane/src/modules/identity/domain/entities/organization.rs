use crate::modules::identity::domain::value_objects::OrganizationName;
use crate::modules::shared_kernel::domain::OrganizationId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Organization {
    pub id: OrganizationId,
    pub name: OrganizationName,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
}

impl Organization {
    pub fn create(id: OrganizationId, name: OrganizationName, created_at: DateTime<Utc>) -> Self {
        Self {
            id,
            name,
            aggregate_version: 1,
            created_at,
        }
    }
}
