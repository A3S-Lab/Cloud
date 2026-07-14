use crate::modules::projects::application::commands::create_project::CreateProjectResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResponse {
    pub organization_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub replayed: bool,
}

impl From<CreateProjectResult> for ProjectResponse {
    fn from(result: CreateProjectResult) -> Self {
        Self {
            organization_id: result.project.organization_id.as_uuid(),
            id: result.project.id.as_uuid(),
            name: result.project.name.as_str().to_owned(),
            aggregate_version: result.project.aggregate_version,
            created_at: result.project.created_at,
            replayed: result.replayed,
        }
    }
}
