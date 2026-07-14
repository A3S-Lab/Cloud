use crate::modules::projects::domain::entities::Environment;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentListItemResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
}

impl From<Environment> for EnvironmentListItemResponse {
    fn from(environment: Environment) -> Self {
        Self {
            organization_id: environment.organization_id.as_uuid(),
            project_id: environment.project_id.as_uuid(),
            id: environment.id.as_uuid(),
            name: environment.name.as_str().to_owned(),
            aggregate_version: environment.aggregate_version,
            created_at: environment.created_at,
        }
    }
}
