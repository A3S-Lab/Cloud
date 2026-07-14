use crate::modules::projects::application::commands::create_environment::CreateEnvironmentResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub replayed: bool,
}

impl From<CreateEnvironmentResult> for EnvironmentResponse {
    fn from(result: CreateEnvironmentResult) -> Self {
        Self {
            organization_id: result.environment.organization_id.as_uuid(),
            project_id: result.environment.project_id.as_uuid(),
            id: result.environment.id.as_uuid(),
            name: result.environment.name.as_str().to_owned(),
            aggregate_version: result.environment.aggregate_version,
            created_at: result.environment.created_at,
            replayed: result.replayed,
        }
    }
}
