use crate::modules::projects::domain::entities::Project;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListItemResponse {
    pub organization_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
}

impl From<Project> for ProjectListItemResponse {
    fn from(project: Project) -> Self {
        Self {
            organization_id: project.organization_id.as_uuid(),
            id: project.id.as_uuid(),
            name: project.name.as_str().to_owned(),
            aggregate_version: project.aggregate_version,
            created_at: project.created_at,
        }
    }
}
