use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::value_objects::PluginRegistryState;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegistryResponse {
    pub organization_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub endpoint: String,
    pub root_object_ref: String,
    pub root_sha256: String,
    pub root_version: u64,
    pub state: PluginRegistryState,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<PluginRegistry> for PluginRegistryResponse {
    fn from(value: PluginRegistry) -> Self {
        Self {
            organization_id: value.organization_id.as_uuid(),
            id: value.id.as_uuid(),
            name: value.name.as_str().to_owned(),
            endpoint: value.endpoint.as_str().to_owned(),
            root_object_ref: value.trust_root.object_ref().as_str().to_owned(),
            root_sha256: value.trust_root.digest().as_str().to_owned(),
            root_version: value.trust_root.version(),
            state: value.state,
            aggregate_version: value.aggregate_version,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}
