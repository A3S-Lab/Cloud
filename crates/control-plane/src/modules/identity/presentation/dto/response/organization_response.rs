use crate::modules::identity::application::commands::create_organization::CreateOrganizationResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationResponse {
    pub id: Uuid,
    pub name: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub replayed: bool,
}

impl From<CreateOrganizationResult> for OrganizationResponse {
    fn from(result: CreateOrganizationResult) -> Self {
        Self {
            id: result.organization.id.as_uuid(),
            name: result.organization.name.as_str().to_owned(),
            aggregate_version: result.organization.aggregate_version,
            created_at: result.organization.created_at,
            replayed: result.replayed,
        }
    }
}
