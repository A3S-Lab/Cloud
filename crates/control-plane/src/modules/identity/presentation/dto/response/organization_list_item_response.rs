use crate::modules::identity::domain::entities::Organization;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationListItemResponse {
    pub id: Uuid,
    pub name: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
}

impl From<Organization> for OrganizationListItemResponse {
    fn from(organization: Organization) -> Self {
        Self {
            id: organization.id.as_uuid(),
            name: organization.name.as_str().to_owned(),
            aggregate_version: organization.aggregate_version,
            created_at: organization.created_at,
        }
    }
}
